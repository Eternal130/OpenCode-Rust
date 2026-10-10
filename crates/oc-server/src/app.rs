//! The app shell: handler trait + registry + the dispatch gate
//! (`process.ts` dispatch + `web-ui.ts` route precedence).
//!
//! Request order (outermost → innermost): **CORS** → **compression** →
//! **dispatch**. The dispatch splits API from assets:
//!
//! - `/api`, `/api/*`, `/auth/*`, `/openapi.json` → readiness gate (only
//!   authed `GET /api/info` passes pre-ready), raw auth gate (pairing
//!   connect always exempt; PTY connect tickets when ready), then the
//!   per-route dispatch. Unmatched API routes answer an empty 404.
//! - everything else → static assets BEFORE auth (the signed-out browser
//!   gets the app shell).

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Request, State};
use axum::http::Method;
use axum::response::Response;
use axum::routing::Router;
use tower::ServiceExt as _;
use tower_http::compression::CompressionLayer;

use crate::assets;
use crate::auth;
use crate::info;
use crate::middleware;
use crate::pairing;
use crate::sse::{self, EventFeed};
use crate::state::{AppState, Readiness};

/// Compression predicate: everything except `text/event-stream` streams
/// (SSE frames must never sit in a compressor buffer).
fn compress_predicate(
    _status: axum::http::StatusCode,
    _version: axum::http::Version,
    headers: &axum::http::HeaderMap,
    _extensions: &axum::http::Extensions,
) -> bool {
    headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_none_or(|content_type| !content_type.starts_with("text/event-stream"))
}

/// A mounted handler invocation context, stashed into request extensions
/// before the registry's handler runs.
#[derive(Debug, Clone)]
pub struct MatchedRoute {
    /// `routes_gen` operation id (e.g. `session.message.get`).
    pub operation_id: &'static str,
    /// Percent-decoded path parameters.
    pub params: Vec<(&'static str, String)>,
}

impl MatchedRoute {
    /// Path parameter by name (percent-decoded).
    #[must_use]
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.iter().find(|(key, _)| *key == name).map(|(_, value)| value.as_str())
    }
}

/// The mount contract the handler lane implements
/// (`async fn call(State<AppState>, Request) -> Response`).
///
/// The request arrives post-auth with [`MatchedRoute`] and the derived
/// [`oc_schema::location::Ref`] in its extensions. Validation failures map
/// to [`errors::schema_rejection`]-style envelopes; engine directory-boot
/// failures map to `LocationNotFoundError` 404 via
/// [`errors::location_not_found_path`].
#[async_trait::async_trait]
pub trait Handler: Send + Sync + 'static {
    /// Handles one API request.
    async fn call(&self, state: Arc<AppState>, request: Request) -> Response;
}

/// Adapter turning an async closure into a [`Handler`].
pub struct HandlerFn<F>(F);

#[async_trait::async_trait]
impl<F, Fut> Handler for HandlerFn<F>
where
    F: Fn(Arc<AppState>, Request) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Response> + Send,
{
    async fn call(&self, state: Arc<AppState>, request: Request) -> Response {
        (self.0)(state, request).await
    }
}

/// Mounts handlers by operation id; the sibling handler lane fills this.
#[derive(Default)]
pub struct HandlerRegistry {
    handlers: dashmap::DashMap<String, Arc<dyn Handler>>,
}

impl HandlerRegistry {
    /// Mounts `handler` for an operation id.
    pub fn mount(&self, operation_id: impl Into<String>, handler: Arc<dyn Handler>) {
        self.handlers.insert(operation_id.into(), handler);
    }

    /// Looks a handler up.
    #[must_use]
    pub fn get(&self, operation_id: &str) -> Option<Arc<dyn Handler>> {
        self.handlers.get(operation_id).map(|entry| Arc::clone(entry.value()))
    }

    /// Whether anything is mounted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }

    /// Number of mounted handlers.
    #[must_use]
    pub fn len(&self) -> usize {
        self.handlers.len()
    }
}

/// The composed app: state, the handler registry, the 141-op handler
/// table, the event feed, and the SSE heartbeat cadence.
pub struct AppShell {
    /// Shared server state.
    pub state: Arc<AppState>,
    /// Handler-lane mounts (operation-id overrides; consulted before the
    /// merged handler table so tests can swap individual operations).
    pub registry: HandlerRegistry,
    /// The handler lane's full 141-operation table (state applied). The
    /// dispatch gate delegates every post-auth API request it does not
    /// serve from its built-ins through this router.
    pub handler_router: Router,
    /// The SSE event feed (bus-tapped).
    pub feed: Arc<EventFeed>,
    /// Heartbeat cadence for `/api/event` (15s in production; tests shrink).
    pub heartbeat: Duration,
}

impl AppShell {
    /// Builds the shell with production defaults (15s heartbeat, live feed
    /// tap over the engine bus).
    #[must_use]
    pub fn new(state: Arc<AppState>) -> Arc<Self> {
        Self::with_heartbeat(state, sse::HEARTBEAT_INTERVAL)
    }

    /// Builds the shell with an explicit SSE heartbeat cadence.
    #[must_use]
    pub fn with_heartbeat(state: Arc<AppState>, heartbeat: Duration) -> Arc<Self> {
        Self::with_feed(state, Arc::new(EventFeed::new()), heartbeat)
    }

    /// Builds the shell around an explicit feed (tests shrink the capacity).
    #[must_use]
    pub fn with_feed(state: Arc<AppState>, feed: Arc<EventFeed>, heartbeat: Duration) -> Arc<Self> {
        feed.spawn_tap(&state.engine.bus);
        crate::announce::ensure_started(&state);
        let handler_router = crate::handlers::mount(Arc::clone(&state));
        Arc::new(Self {
            state,
            registry: HandlerRegistry::default(),
            handler_router,
            feed,
            heartbeat,
        })
    }

    /// Mounts a handler for an operation id (builder-style).
    pub fn mount(&self, operation_id: impl Into<String>, handler: Arc<dyn Handler>) -> &Self {
        self.registry.mount(operation_id, handler);
        self
    }

    /// Builds the axum router: CORS outermost, then compression, then the
    /// dispatch fallback.
    pub fn router(self: &Arc<Self>) -> Router {
        let cors: Arc<Vec<String>> = Arc::new(self.state.cors_origins.clone());
        Router::new()
            .fallback(dispatch)
            // SSE must never be compressed (frame buffering).
            .layer(CompressionLayer::new().compress_when(compress_predicate))
            .layer(axum::middleware::from_fn_with_state(cors, middleware::cors_middleware))
            .with_state(Arc::clone(self))
    }
}

/// The dispatch gate (`process.ts dispatch`).
async fn dispatch(State(shell): State<Arc<AppShell>>, request: Request) -> Response {
    let state = &shell.state;
    let method = request.method().clone();
    let uri = request.uri().clone();
    let path = uri.path().to_owned();
    let headers = request.headers().clone();

    // Web UI first: everything that is not API surface is served BEFORE auth.
    let is_api = path == "/api"
        || path.starts_with("/api/")
        || path.starts_with("/auth/")
        || path == "/openapi.json";
    if !is_api {
        return assets::serve(&path, &method, state);
    }

    let readiness = state.readiness();
    let ready = readiness == Readiness::Ready;

    // Pre-ready: only authed GET /api/info passes (no URL exemptions here).
    if !ready && method == Method::GET && path == "/api/info" {
        if !auth::request_authorized(&uri, &headers, state) {
            return auth::unauthorized_response(&headers);
        }
        return info::info_response(state);
    }

    // Raw auth gate with exemptions (pairing connect always; PTY tickets
    // only once ready).
    if !middleware::dispatch_gate_passes(&uri, &headers, state, ready) {
        return auth::unauthorized_response(&headers);
    }

    if !ready {
        return info::unavailable_response(readiness);
    }

    let response = dispatch_ready(&shell, request).await;
    // The location announcer: after every dispatched API request, publish
    // boot cascades for newly-assembled locations (request-driven so the
    // config baseline is captured synchronously at boot — see announce).
    crate::announce::after_request(state).await;
    response
}

/// Post-gate routing: built-in app-shell routes, then the handler registry,
/// then the empty 404 (`RouteNotFound`).
async fn dispatch_ready(shell: &Arc<AppShell>, request: Request) -> Response {
    let state = &shell.state;
    let method = request.method().clone();
    let uri = request.uri().clone();
    let path = uri.path().to_owned();

    if path == "/api/info" && method == Method::GET {
        return info::info_response(state);
    }
    if path == "/api/pair" && method == Method::POST {
        return pairing::pair_response(state);
    }
    if path == "/api/event" && method == Method::GET {
        return sse::event_response(state, &shell.feed, shell.heartbeat);
    }
    if path == "/openapi.json" && method == Method::GET {
        return assets::openapi_response();
    }
    if let Some(code) = path.strip_prefix("/auth/connect/") {
        if method == Method::GET && !code.is_empty() && !code.contains('/') {
            let code = auth::percent_decode(code);
            return pairing::connect_response(state, &code, request.headers());
        }
    }
    if let Some(rest) = path.strip_prefix("/api/experimental/session/") {
        if let Some(id) = rest.strip_suffix("/log") {
            if method == Method::GET && !id.is_empty() && !id.contains('/') {
                let headers = request.headers().clone();
                return sse::session_log_response(state, &uri, &headers, id).await;
            }
        }
    }

    // Handler registry over the generated route table (explicit mounts
    // win — the override seam the shell contract documents).
    let table_method = match method {
        Method::GET => Some(crate::routes_gen::Method::Get),
        Method::POST => Some(crate::routes_gen::Method::Post),
        Method::PUT => Some(crate::routes_gen::Method::Put),
        Method::DELETE => Some(crate::routes_gen::Method::Delete),
        Method::PATCH => Some(crate::routes_gen::Method::Patch),
        _ => None,
    };
    if let Some(table_method) = table_method {
        if let Some((route, params)) = crate::routes_gen::find(table_method, &path) {
            if let Some(handler) = shell.registry.get(route.operation_id) {
                let location = middleware::request_location(&uri, request.headers());
                let mut request = request;
                request.extensions_mut().insert(MatchedRoute {
                    operation_id: route.operation_id,
                    params: params
                        .into_iter()
                        .map(|(name, value)| (name, auth::percent_decode(&value)))
                        .collect(),
                });
                request.extensions_mut().insert(location);
                return handler.call(Arc::clone(state), request).await;
            }
        }
    }

    // The merged 141-op handler table: every remaining API route is served
    // by the handler lane's axum handlers (they read the location from the
    // query/headers themselves; the delegation wrappers for the shell
    // built-ins above stay mounted in the table for standalone use but are
    // unreachable here — the built-ins win, exactly once per route).
    // `Infallible` service error: the result cannot be an error.
    let response =
        shell.handler_router.clone().oneshot(request).await.unwrap_or_else(|never| match never {});
    // P10-E3: v2's Effect router has no 405 — a method mismatch on a
    // known path answers the same empty-body 404 as an unknown route
    // (axum's default method-not-allowed is rewritten; the web-UI asset
    // 405 is a different seam, served before this one).
    if response.status() == axum::http::StatusCode::METHOD_NOT_ALLOWED {
        return crate::errors::empty(axum::http::StatusCode::NOT_FOUND);
    }
    response
}
