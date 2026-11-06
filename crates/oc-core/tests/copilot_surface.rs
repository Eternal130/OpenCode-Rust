//! G-167 — the copilot dynamic model surface: the live `/models` merge
//! over the static catalog, the Anthropic escape lane for `/v1/messages`
//! models, the hidden `gpt-5-chat-latest` alias, resolver wiring, and
//! the free utility-title hook.
//!
//! Hermetic: the copilot sync is seeded through its offline seam
//! (`CopilotSync::seed_for_test` — the production fetch never runs); the
//! utility-title call hits a local stub on `127.0.0.1:0`.

use std::collections::BTreeMap;
use std::sync::Arc;

use oc_core::database::schema::SessionRow;
use oc_core::engine::resolver::CatalogModelResolver;
use oc_core::provider_catalog::catalog::Catalog;
use oc_core::provider_catalog::copilot_sync::{
    merge_copilot_snapshot, CopilotSync, CopilotUtilityTitleHook, HIDDEN_COPILOT_MODEL,
};
use oc_core::provider_catalog::facade::{language_model_for_model, FacadeContext, ModelConnection};
use oc_core::session::runner::ModelResolver as _;
use oc_ai::protocols::copilot::CopilotRemoteModel;
use serde_json::json;

fn remote_model(id: &str, endpoints: &[&str]) -> CopilotRemoteModel {
    serde_json::from_value(json!({
        "model_picker_enabled": true,
        "id": id,
        "name": format!("Model {id}"),
        "version": format!("{id}-2026-03-05"),
        "supported_endpoints": endpoints,
        "capabilities": {
            "family": "test-family",
            "limits": { "max_output_tokens": 4096, "max_prompt_tokens": 100_000 },
            "supports": { "tool_calls": true }
        }
    }))
    .expect("remote model decodes")
}

fn transport() -> oc_ai::route::SharedTransport {
    Arc::new(oc_ai::route::transport::reqwest_transport::ReqwestTransport::new())
}

fn session_row() -> SessionRow {
    SessionRow::new("ses_copilot_surface", "global", "/tmp/opencode", "copilot-surface", "test")
        .expect("session row")
}

fn model_ref(provider: &str, model: &str) -> oc_schema::model::Ref {
    oc_schema::model::Ref {
        provider_id: oc_schema::id::ProviderId::parse(provider.to_owned()),
        id: oc_schema::id::ModelId::parse(model.to_owned()),
        variant: None,
    }
}

/// The merge (github-copilot.ts:216-234 over models.ts `derive`): kept
/// models rebuild from remote facts (keeping their local name/family),
/// stale entries drop, unseen remote models join with the full derived
/// `Model.Info` — and `gpt-5-chat-latest` hides even though the remote
/// listing enables it (github-copilot.ts:259-268).
#[test]
fn merge_folds_remote_over_static() {
    let catalog = Catalog::embedded();
    let static_snapshot = catalog.provider("github-copilot").expect("copilot in catalog");
    let kept_id = static_snapshot.models[0].id.0.clone();
    let dropped_id = static_snapshot.models[1].id.0.clone();

    // A static alias entry pointing at the kept model's API id.
    let mut aliased = static_snapshot.clone();
    let alias = oc_schema::model::Info {
        id: oc_schema::id::ModelId::parse("my-alias"),
        model_id: static_snapshot.models[0].model_id.clone(),
        name: "My Local Name".to_string(),
        family: Some("local-family".to_string()),
        ..oc_schema::model::Info::default_for(
            oc_schema::id::ProviderId::parse("github-copilot"),
            oc_schema::id::ModelId::parse("my-alias"),
        )
    };
    let hidden = oc_schema::model::Info {
        enabled: true,
        ..oc_schema::model::Info::default_for(
            oc_schema::id::ProviderId::parse("github-copilot"),
            oc_schema::id::ModelId::parse(HIDDEN_COPILOT_MODEL),
        )
    };
    aliased.models.push(alias);
    aliased.models.push(hidden);

    let remote = BTreeMap::from([
        (static_snapshot.models[0].model_id.0.clone(), remote_model(&kept_id, &["/chat/completions"])),
        (HIDDEN_COPILOT_MODEL.to_string(), remote_model(HIDDEN_COPILOT_MODEL, &["/chat/completions"])),
        ("brand-new-model".to_string(), remote_model("brand-new-model", &["/responses"])),
    ]);
    let merged = merge_copilot_snapshot(&aliased, "https://api.githubcopilot.com", &remote);

    let by_id: BTreeMap<&str, _> =
        merged.models.iter().map(|model| (model.id.as_str(), model)).collect();
    assert!(!by_id.contains_key(dropped_id.as_str()), "a model whose API id left the listing drops");
    assert!(by_id.contains_key("brand-new-model"), "an unseen remote model joins");
    assert!(by_id.contains_key("my-alias"), "an alias whose API model survives keeps");

    // The alias keeps its local name/family but takes the remote
    // endpoint/baseURL overlays (models.ts:103-108, 158-166).
    let alias = by_id["my-alias"];
    assert_eq!(alias.name, "My Local Name");
    assert_eq!(alias.family.as_deref(), Some("local-family"));
    assert_eq!(
        alias.settings.as_ref().expect("settings").rest.get("baseURL"),
        Some(&json!("https://api.githubcopilot.com"))
    );

    // The new model carries the derived fields (G-167.3).
    let new_model = by_id["brand-new-model"];
    assert_eq!(new_model.family.as_deref(), Some("test-family"));
    assert_eq!(new_model.capabilities.input, vec!["text"]);
    assert_eq!(new_model.limit.context, 100_000);
    assert_eq!(new_model.limit.output, 4_096);
    assert!(new_model.enabled, "model_picker_enabled=true keeps it available");
    // Released: version `{id}-2026-03-05` strips to the ISO date.
    assert_eq!(new_model.time.released.0, 1_772_668_800_000.0);

    // The conflicting alias hides regardless of its picker flag.
    assert!(!by_id[HIDDEN_COPILOT_MODEL].enabled, "gpt-5-chat-latest hides for copilot");
}

/// G-167.1 — the Anthropic escape lane: a `/v1/messages` model merges
/// with the anthropic package and binds through the anthropic-messages
/// machinery on the copilot `{base}/v1` root with the copilot bearer
/// token — never the chat/completions dispatch.
#[tokio::test]
async fn anthropic_escape_lane_binds_messages_route() {
    let catalog = Catalog::embedded();
    let static_snapshot = catalog.provider("github-copilot").expect("copilot in catalog");
    let remote = BTreeMap::from([(
        "claude-sonnet-4-5".to_string(),
        remote_model("claude-sonnet-4-5", &["/v1/messages", "/chat/completions"]),
    )]);
    let merged = merge_copilot_snapshot(static_snapshot, "https://copilot.example", &remote);
    let claude = merged
        .models
        .iter()
        .find(|model| model.id.as_str() == "claude-sonnet-4-5")
        .expect("the claude model joined");
    assert_eq!(
        claude.package.as_deref(),
        Some("@opencode/ai/providers/anthropic"),
        "models.ts:164 — the package rewrite"
    );
    assert_eq!(
        claude.settings.as_ref().expect("settings").rest.get("baseURL"),
        Some(&json!("https://copilot.example/v1"))
    );

    let context = FacadeContext::new(transport());
    let model = language_model_for_model(
        &context,
        &merged,
        claude,
        &ModelConnection {
            base_url: Some("https://copilot.example".to_string()),
            api_key: Some("ghu_copilot_token".to_string()),
            ..Default::default()
        },
    )
    .expect("the messages lane binds");
    assert_eq!(model.route.id, "copilot-messages");
    assert_eq!(
        model.route.endpoint.base_url.as_deref(),
        Some("https://copilot.example/v1"),
        "the anthropic root is the copilot /v1 base"
    );
    let route = Arc::clone(&model.route);
    let mut request = oc_ai::schema::LLMRequest::new(model);
    request.messages = vec![oc_ai::schema::Message::user("hi")];
    let prepared = route.prepare_request(&request);
    let body = route.build_body(&prepared).expect("body");
    let (wire, _) = route.prepare_transport(&prepared, &body).expect("wire");
    assert!(wire.url.ends_with("/v1/messages"), "the wire hits /v1/messages: {}", wire.url);
    assert!(!wire.url.contains("chat/completions"), "{}", wire.url);
    assert_eq!(wire.header("authorization"), Some("Bearer ghu_copilot_token"));
}

/// G-167.2 — production resolution serves the dynamic surface: the
/// resolver with a synced copilot state binds models that exist ONLY in
/// the live listing (an enterprise-only model here), and a static model
/// whose API id left the listing stops resolving.
#[tokio::test]
async fn resolver_binds_dynamic_models() {
    let catalog = Arc::new(Catalog::embedded().clone());
    let sync = Arc::new(CopilotSync::new(transport()));
    sync.seed_for_test(
        &catalog,
        "https://api.githubcopilot.com",
        "ghu_token",
        BTreeMap::from([(
            "enterprise-only-model".to_string(),
            remote_model("enterprise-only-model", &["/chat/completions"]),
        )]),
    );
    let resolver = CatalogModelResolver::new(
        FacadeContext::new(transport()),
        catalog,
        BTreeMap::new(),
        Vec::new(),
        None,
    )
    .with_copilot_sync(Arc::clone(&sync));

    let session = session_row();
    let bound = resolver
        .resolve_selected(&session, &model_ref("github-copilot", "enterprise-only-model"))
        .expect("the dynamic-only model resolves");
    assert_eq!(bound.model.route.id, "copilot-chat");

    // The static-only ids dropped from the listing no longer resolve —
    // every static copilot model is absent from the seeded snapshot.
    let gone = resolver
        .resolve_selected(&session, &model_ref("github-copilot", "gpt-5.4"))
        .expect_err("a model absent from the live listing fails");
    assert!(matches!(gone, oc_core::session::runner::ModelError::Unavailable(_, _)));
}

/// Without a sync the static catalog still serves copilot, and the
/// conflicting `gpt-5-chat-latest` alias hides in resolution.
#[tokio::test]
async fn static_catalog_serves_without_sync() {
    let catalog = Arc::new(Catalog::embedded().clone());
    let resolver = CatalogModelResolver::new(
        FacadeContext::new(transport()),
        Arc::clone(&catalog),
        BTreeMap::new(),
        Vec::new(),
        None,
    );
    let session = session_row();
    resolver
        .resolve_selected(&session, &model_ref("github-copilot", "gpt-5.4"))
        .expect("the static catalog still binds copilot");

    // The hidden alias is disabled in the merged listing even when the
    // remote listing enables it (github-copilot.ts:259-268).
    let static_snapshot = catalog.provider("github-copilot").expect("copilot");
    let remote = BTreeMap::from([(
        HIDDEN_COPILOT_MODEL.to_string(),
        remote_model(HIDDEN_COPILOT_MODEL, &["/chat/completions"]),
    )]);
    let merged = merge_copilot_snapshot(static_snapshot, "https://api.githubcopilot.com", &remote);
    let hidden = merged
        .models
        .iter()
        .find(|model| model.id.as_str() == HIDDEN_COPILOT_MODEL)
        .expect("the alias merged");
    assert!(!hidden.enabled);
}

/// G-167.5 — the free-title hook: a copilot session's title request is
/// served by the gpt-4o-mini utility channel (its own
/// `/chat/completions` call, non-streaming, the utility interaction
/// headers), and any failure leaves `result` unset so the billable path
/// still runs.
#[allow(clippy::too_many_lines, reason = "one wired scenario per assertion block")]
#[tokio::test]
async fn free_title_hook_uses_the_utility_channel() {
    use oc_core::hooks::{HookOptions, HookRegistry, SessionTitleEvent};

    // The utility completion stub.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("port").port();
    let captured = Arc::new(std::sync::Mutex::new(String::new()));
    tokio::spawn({
        let captured = Arc::clone(&captured);
        async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let captured = Arc::clone(&captured);
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
                    let mut buffer = vec![0u8; 65536];
                    let read = stream.read(&mut buffer).await.unwrap_or(0);
                    *captured.lock().unwrap() =
                        String::from_utf8_lossy(&buffer[..read]).into_owned();
                    let body = r#"{"choices":[{"message":{"content":"Utility title"}}]}"#;
                    let reply = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(reply.as_bytes()).await;
                });
            }
        }
    });

    let catalog = Arc::new(Catalog::embedded().clone());
    let base_url = format!("http://127.0.0.1:{port}");
    let sync = Arc::new(CopilotSync::new(transport()));
    sync.seed_for_test(
        &catalog,
        &base_url,
        "ghu_utility",
        BTreeMap::from([
            ("gpt-4o-mini".to_string(), remote_model("gpt-4o-mini", &["/chat/completions"])),
            ("gpt-5.4".to_string(), remote_model("gpt-5.4", &["/responses"])),
        ]),
    );

    let hooks = HookRegistry::new();
    let _keep = hooks
        .register_session_title(
            Arc::new(CopilotUtilityTitleHook::new(
                Arc::clone(&sync),
                transport(),
                "opencode-test",
            )),
            HookOptions { provider_id: Some("github-copilot".to_string()) },
        )
        .await;

    let event = SessionTitleEvent {
        request: oc_core::hooks::SessionRequestEvent {
            session_id: oc_schema::id::SessionId::parse("ses_utility".to_owned()).expect("session id"),
            model: model_ref("github-copilot", "gpt-5.4"),
            system: vec![oc_ai::schema::messages::SystemPart::new("name the session")],
            messages: vec![oc_ai::schema::Message::user("port a compiler")],
            options: serde_json::Map::new(),
        },
        result: None,
    };
    let shaped = hooks.dispatch_session_title(event).await.expect("dispatch");
    assert_eq!(shaped.result.as_deref(), Some("Utility title"));

    let request = captured.lock().unwrap().clone();
    let lowered = request.to_ascii_lowercase();
    assert!(lowered.contains("x-interaction-type: agent-session-name-generation"), "{request}");
    assert!(lowered.contains("authorization: bearer ghu_utility"), "{request}");
    let body = request.split("\r\n\r\n").nth(1).unwrap_or_default();
    let parsed: serde_json::Value = serde_json::from_str(body).expect("body is json");
    assert_eq!(parsed["model"], "gpt-4o-mini", "the FREE utility model, not the session model");
    assert_eq!(parsed["stream"], false);

    // A non-copilot session never hits the channel (the provider filter).
    let other = SessionTitleEvent {
        request: oc_core::hooks::SessionRequestEvent {
            session_id: oc_schema::id::SessionId::parse("ses_other".to_owned()).expect("session id"),
            model: model_ref("anthropic", "claude-opus-4-5"),
            system: vec![],
            messages: vec![oc_ai::schema::Message::user("x")],
            options: serde_json::Map::new(),
        },
        result: None,
    };
    let shaped = hooks.dispatch_session_title(other).await.expect("dispatch");
    assert_eq!(shaped.result, None, "non-copilot sessions take the billable path");

    // An account without gpt-4o-mini falls through too.
    let sync_sans_utility = Arc::new(CopilotSync::new(transport()));
    sync_sans_utility.seed_for_test(
        &catalog,
        &base_url,
        "ghu_utility",
        BTreeMap::from([("gpt-5.4".to_string(), remote_model("gpt-5.4", &["/responses"]))]),
    );
    let event = SessionTitleEvent {
        request: oc_core::hooks::SessionRequestEvent {
            session_id: oc_schema::id::SessionId::parse("ses_sans".to_owned()).expect("session id"),
            model: model_ref("github-copilot", "gpt-5.4"),
            system: vec![],
            messages: vec![oc_ai::schema::Message::user("x")],
            options: serde_json::Map::new(),
        },
        result: None,
    };
    let hook = CopilotUtilityTitleHook::new(sync_sans_utility, transport(), "opencode-test");
    let mut shaped = event;
    oc_core::hooks::HookHandler::call(
        &hook as &dyn oc_core::hooks::HookHandler<SessionTitleEvent>,
        &mut shaped,
    )
    .await
    .expect("handler");
    assert_eq!(shaped.result, None, "no utility model → no free title");
}
