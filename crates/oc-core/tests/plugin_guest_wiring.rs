//! PLUG-M02 — the transport-layer hook dispatch: `session.http.request`
//! / `session.http.response` wrap every HTTP request the route executes
//! (v2 model-request.ts:320-348), and the `experimental.ws.*` seams are
//! offered for a durable session's stateful socket (349-375).
//!
//! The http pair is proven end-to-end through the PRODUCTION pipeline:
//! `SessionModelRequest::prepare` wraps the request's route transport,
//! the route's framing sees the REWRITTEN request, and the response hook
//! observes/rewrites the answer. Hermetic: an oc-ai `MockTransport`
//! (no network), an in-memory row, native hooks (the guest bridge rides
//! the same registry dispatch).

use std::sync::Arc;

use oc_ai::test_support::{echo_route, MockTransport, RecordedInteraction};
use oc_core::hooks::{
    HookRegistry, HttpRequest, HttpResponse, SessionHttpRequestEvent, SessionHttpResponseEvent,
};
use oc_core::session::runner::model_request::{RequestInput, RequestKind, SessionModelRequest};
use oc_core::session::runner::ResolvedModel;
use serde_json::{json, Value};

/// A resolved model over the echo route (the catalog facts the
/// preparation reads are all empty defaults).
fn resolved_model(route: &Arc<oc_ai::route::Route>) -> ResolvedModel {
    ResolvedModel {
        model: route.model("hooked-model", None).expect("model"),
        reference: oc_schema::model::Ref {
            provider_id: oc_schema::id::ProviderId::parse("echo-provider"),
            id: oc_schema::id::ModelId::parse("hooked-model"),
            variant: None,
        },
        capabilities: oc_schema::model::Capabilities::default(),
        cost: Vec::new(),
        limit: oc_schema::model::ModelLimit { context: 0, input: None, output: 0 },
        compaction: None,
        transport: None,
    }
}

#[tokio::test]
async fn http_hook_dispatch() {
    let hooks = Arc::new(HookRegistry::new());
    let tools = Arc::new(oc_core::location::ToolRegistryState::new());

    // The plugin-shaped pair: the request hook stamps a header + rewrites
    // the body's prompt; the response hook records what the route saw and
    // rewrites a header. Mutations must flow through the transport.
    let seen_requests: Arc<std::sync::Mutex<Vec<HttpRequest>>> = Arc::default();
    let seen_responses: Arc<std::sync::Mutex<Vec<HttpResponse>>> = Arc::default();
    let request_seen = Arc::clone(&seen_requests);
    let registration = hooks
        .register_session_http_request(
            oc_core::hooks::fn_hook(move |event: &mut SessionHttpRequestEvent| {
                request_seen.lock().unwrap().push(event.request.clone());
                event.request.headers.insert("x-plugin-tweak".to_owned(), "1".to_owned());
                if let Value::Object(body) = &mut event.request.body {
                    body.insert("prompt".to_owned(), json!("rewritten-by-plugin"));
                }
                Ok(())
            }),
            oc_core::hooks::HookOptions::default(),
        )
        .await;
    // The plugin registration lives for the location's lifetime.
    std::mem::forget(registration);
    let response_seen = Arc::clone(&seen_responses);
    let registration = hooks
        .register_session_http_response(
            oc_core::hooks::fn_hook(move |event: &mut SessionHttpResponseEvent| {
                response_seen.lock().unwrap().push(event.response.clone());
                event
                    .response
                    .headers
                    .insert("x-plugin-response".to_owned(), "observed".to_owned());
                Ok(())
            }),
            oc_core::hooks::HookOptions::default(),
        )
        .await;
    std::mem::forget(registration);

    let transport = Arc::new(MockTransport::from_interactions(vec![RecordedInteraction {
        method: "POST".into(),
        url: "https://echo.test/echo".into(),
        status: 200,
        body: "data: ok\n\n".into(),
        request_body: None,
    }]));
    let route = Arc::new(echo_route(transport.clone() as oc_ai::route::transport::SharedTransport));
    let model = resolved_model(&route);

    let session = oc_core::database::schema::SessionRow::new(
        "ses_http_hook",
        "global",
        "/tmp/opencode",
        "http-hook",
        "test",
    )
    .expect("session row");

    let service = SessionModelRequest::new(
        Arc::clone(&hooks),
        tools,
        oc_core::session::runner::model_request::AppMetadata::default(),
    );
    let prepared = service
        .prepare(
            RequestKind::Primary,
            RequestInput {
                session: session.clone(),
                agent: oc_schema::id::AgentId::parse("build"),
                model,
                tools: Some(Vec::new()),
                system: Vec::new(),
                messages: vec![oc_ai::schema::messages::Message::user("hi")],
                tool_choice: None,
                web_socket: false,
                input_tokens: None,
            },
        )
        .await
        .expect("prepare");

    // The wrapped route streams: the wrapped transport dispatched both
    // hooks before the framing saw anything.
    let response = Arc::clone(&prepared.request.model.route)
        .generate(prepared.request)
        .await
        .expect("generate through the wrapped transport");
    assert_eq!(response.text(), "ok", "the rewritten body still parses");

    // The outbound request the transport executed carries the request
    // hook's mutations — this is the wrap, not a copy.
    let sent = &transport.requests()[0];
    assert_eq!(sent.header("x-plugin-tweak"), Some("1"), "the hook's header rode the wire");
    let body: Value = serde_json::from_slice(&sent.body).expect("body is JSON");
    assert_eq!(body["prompt"], json!("rewritten-by-plugin"), "the hook rewrote the body");

    // Both hooks observed the real exchange.
    {
        let requests = seen_requests.lock().unwrap();
        assert_eq!(requests.len(), 1, "one request event");
        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].url, "https://echo.test/echo");
    }
    {
        let responses = seen_responses.lock().unwrap();
        assert_eq!(responses.len(), 1, "one response event");
        assert_eq!(responses[0].status, 200);
        assert_eq!(
            responses[0].body,
            Value::String("data: ok\n\n".to_owned()),
            "SSE text bodies cross as text, not forced JSON"
        );
    }
}

/// The no-hooks path stays UNWRAPPED: the route's own transport streams
/// unchanged (v2's `hasHttpHooks` gate — buffering only ever happens for
/// a registered pair).
#[tokio::test]
async fn no_hooks_leaves_the_transport_unwrapped() {
    let hooks = Arc::new(HookRegistry::new());
    assert!(!hooks.has_session_http_request(None));
    assert!(!hooks.has_session_http_response(None));
    let transport = Arc::new(MockTransport::from_interactions(vec![RecordedInteraction {
        method: "POST".into(),
        url: "https://echo.test/echo".into(),
        status: 200,
        body: "data: ok\n\n".into(),
        request_body: None,
    }]));
    let route = Arc::new(echo_route(transport.clone() as oc_ai::route::transport::SharedTransport));
    let model = resolved_model(&route);
    let session = oc_core::database::schema::SessionRow::new(
        "ses_http_hook2",
        "global",
        "/tmp/opencode",
        "http-hook",
        "test",
    )
    .expect("session row");

    let service = SessionModelRequest::new(
        Arc::clone(&hooks),
        Arc::new(oc_core::location::ToolRegistryState::new()),
        oc_core::session::runner::model_request::AppMetadata::default(),
    );
    let prepared = service
        .prepare(
            RequestKind::Primary,
            RequestInput {
                session,
                agent: oc_schema::id::AgentId::parse("build"),
                model,
                tools: Some(Vec::new()),
                system: Vec::new(),
                messages: vec![oc_ai::schema::messages::Message::user("hi")],
                tool_choice: None,
                web_socket: false,
                input_tokens: None,
            },
        )
        .await
        .expect("prepare");

    let response =
        Arc::clone(&prepared.request.model.route).generate(prepared.request).await.expect("stream");
    assert_eq!(response.text(), "ok");
    let sent = &transport.requests()[0];
    assert!(
        sent.header("x-plugin-tweak").is_none(),
        "no hook ran — the bare transport served the request"
    );
}

/// The ws seams (model-request.ts:351-375): offered only for a durable
/// session's stateful socket AND only when hooks registered — the seam
/// closures dispatch the `experimental.ws.*` events when the channel
/// executor invokes them.
#[tokio::test]
async fn ws_seams_offered_for_hooked_durable_sessions() {
    let hooks = Arc::new(HookRegistry::new());
    let registration = hooks
        .register_session_ws_send(
            oc_core::hooks::fn_hook(|event: &mut oc_core::hooks::SessionWsSendEvent| {
                event.frame = format!("{}|touched-by-plugin", event.frame);
                Ok(())
            }),
            oc_core::hooks::HookOptions::default(),
        )
        .await;
    std::mem::forget(registration);
    let service = SessionModelRequest::new(
        Arc::clone(&hooks),
        Arc::new(oc_core::location::ToolRegistryState::new()),
        oc_core::session::runner::model_request::AppMetadata::default(),
    );
    let session = oc_core::database::schema::SessionRow::new(
        "ses_ws_hook",
        "global",
        "/tmp/opencode",
        "ws-hook",
        "test",
    )
    .expect("session row");
    let prepare = |web_socket: bool| {
        let service = &service;
        let session = session.clone();
        async move {
            service
                .prepare(
                    RequestKind::Primary,
                    RequestInput {
                        session,
                        agent: oc_schema::id::AgentId::parse("build"),
                        model: plain_model(),
                        tools: Some(Vec::new()),
                        system: Vec::new(),
                        messages: vec![oc_ai::schema::messages::Message::user("hi")],
                        tool_choice: None,
                        web_socket,
                        input_tokens: None,
                    },
                )
                .await
                .expect("prepare")
        }
    };

    // Durable session + a registered ws hook: the seams are offered, and
    // invoking one dispatches the hook (the frame rewrite flows back).
    let durable = prepare(true).await;
    let seams = durable.web_socket_seams.expect("hooks registered → seams offered");
    let frame = (seams.send)("raw-frame".to_owned()).await.expect("the send seam dispatches");
    assert_eq!(frame, "raw-frame|touched-by-plugin", "the ws.send rewrite rode the seam");

    // A non-durable request never offers the seams.
    let streamed = prepare(false).await;
    assert!(streamed.web_socket_seams.is_none(), "web_socket=false → no seams");

    // No hooks at all: never offered.
    let bare = SessionModelRequest::new(
        Arc::new(HookRegistry::new()),
        Arc::new(oc_core::location::ToolRegistryState::new()),
        oc_core::session::runner::model_request::AppMetadata::default(),
    );
    let offered = bare
        .prepare(
            RequestKind::Primary,
            RequestInput {
                session: session.clone(),
                agent: oc_schema::id::AgentId::parse("build"),
                model: plain_model(),
                tools: Some(Vec::new()),
                system: Vec::new(),
                messages: vec![oc_ai::schema::messages::Message::user("hi")],
                tool_choice: None,
                web_socket: true,
                input_tokens: None,
            },
        )
        .await
        .expect("prepare");
    assert!(offered.web_socket_seams.is_none(), "no hooks → no seams");
}

/// A model handle over a failing route (the ws leg never streams).
fn plain_model() -> ResolvedModel {
    let route = oc_ai::test_support::echo_route_named("ws-hooks");
    ResolvedModel {
        model: route.model("hooked-model", None).expect("model"),
        reference: oc_schema::model::Ref {
            provider_id: oc_schema::id::ProviderId::parse("test"),
            id: oc_schema::id::ModelId::parse("hooked-model"),
            variant: None,
        },
        capabilities: oc_schema::model::Capabilities::default(),
        cost: Vec::new(),
        limit: oc_schema::model::ModelLimit { context: 0, input: None, output: 0 },
        compaction: None,
        transport: None,
    }
}
