//! Route-coverage manifest test: every one of the 141 `routes_gen`
//! operations must be mounted on the handler table. Each operation gets a
//! probe request through the real router (axum `tower::Service::oneshot`);
//! an UNMOUNTED route would fall through to axum's default fallback —
//! an empty 404 — while every mounted handler answers with a body
//! (JSON envelope, error text, or a schema rejection).

use std::sync::Arc;

use axum::http::{Request, StatusCode};
use oc_core::engine::{Engine, EngineOptions};
use oc_server::handlers;
use oc_server::routes_gen::{self, Method, Route};
use oc_server::state::{AppState, AuthConfig};
use oc_util::determinism::{SystemClock, UuidV4IdGen};

async fn router() -> axum::Router {
    let dir = tempfile::tempdir().expect("tempdir");
    std::env::set_current_dir(dir.path()).expect("chdir to workspace");
    let engine =
        Engine::boot(EngineOptions::new(dir.path().join("data"), dir.path().join("home"))).await;
    let state = Arc::new(AppState::new(
        engine,
        AuthConfig { password: None },
        "test-version".to_owned(),
        Arc::new(UuidV4IdGen),
        Arc::new(SystemClock),
    ));
    let _ = state.readiness.send(oc_server::state::Readiness::Ready);
    // Keep the engine's files alive past the tempdir drop for the probe.
    std::mem::forget(dir);
    handlers::mount(state)
}

/// Substitutes sample values for path params.
fn concrete_path(route: &Route, params: &[(&'static str, String)]) -> String {
    let mut path = route.path.to_owned();
    for (name, value) in params {
        path = path.replace(&format!(":{name}"), value);
        path = path.replace(&format!("*{name}"), value);
    }
    path
}

fn sample_params(route: &Route) -> Vec<(&'static str, String)> {
    route
        .path_params
        .iter()
        .map(|param| {
            let value = match param.name {
                "sessionID" => "ses_probe1",
                "messageID" => "msg_probe1",
                "formID" => "frm_probe1",
                "inboxID" => "msg_probe2",
                "requestID" => "per_probe1",
                "key" => "probe-key",
                "path" => "a/b.txt",
                "code" => "probe-code",
                _ => "probe1",
            };
            (param.name, value.to_owned())
        })
        .collect()
}

#[tokio::test]
async fn handlers_all_141_operations_mounted() {
    let mut router = router().await;
    let mut unmounted = Vec::new();

    for route in routes_gen::ROUTES {
        let path = concrete_path(route, &sample_params(route));
        let method = match route.method {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Delete => "DELETE",
            Method::Patch => "PATCH",
        };
        let body = if route.request_schema.is_empty() {
            axum::body::Body::empty()
        } else {
            // A JSON body satisfies the typed extractors (handlers may
            // still reject the contents — that is a mounted response).
            axum::body::Body::from("{}")
        };
        let request = Request::builder()
            .method(method)
            .uri(&path)
            .header("content-type", "application/json")
            .body(body)
            .expect("request");
        let response = tower::Service::call(&mut router, request).await.expect("route");
        let status = response.status();
        if route.operation_id == "event.subscribe" {
            // The SSE feed streams forever; a 200 + content-type is the
            // mounted proof.
            assert_eq!(status, StatusCode::OK, "event.subscribe must mount");
            let content_type = response
                .headers()
                .get("content-type")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_owned();
            assert!(content_type.starts_with("text/event-stream"), "sse content-type");
            continue;
        }
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024).await.unwrap_or_default();
        // The fallback 404 is byte-empty; every mounted handler answers
        // with a body (error envelope, schema rejection, or payload).
        if status == StatusCode::NOT_FOUND && bytes.is_empty() {
            unmounted.push(format!("{} {} ({})", method, path, route.operation_id));
        }
    }

    assert!(
        unmounted.is_empty(),
        "unmounted operations ({} of {}): {unmounted:?}",
        unmounted.len(),
        routes_gen::OPERATION_COUNT,
    );
    assert_eq!(routes_gen::OPERATION_COUNT, 141);
}
