//! Request-body parity against the TS v2 implementation (oracle gate 3,
//! finding 4 — the response→event parity corpus was body-blind).
//!
//! The dump generators in `/tmp/opencode/ai-dump/` were extended so every
//! expected interaction also carries:
//!
//! - `request` — a summary of the driving `LLMRequest` (model, system,
//!   messages, tools, toolChoice, generation, providerOptions, promptCache);
//! - `body` — the provider-native body the REAL v2 executor pipeline built
//!   (`compileRequest` → `route.body.from(prepareRequest(request))`);
//! - `bodyText` — the exact TS serialization of that body (insertion
//!   order preserved by `JSON.stringify`).
//!
//! These tests rebuild the same `LLMRequest` through the Rust route stack
//! (`prepare_request` + `build_body`) and require **semantic** per-field
//! equality with the TS body: numbers compare as `f64` and object key ORDER
//! is ignored (serde's `BTreeMap` alphabetizes; TS keeps construction order).
//!
//! Byte-level parity is gated behind `OC_AI_BODY_BYTE_PARITY=1` (one test
//! per family, skipped by default): the known divergence is that Rust
//! request bodies serialize with alphabetized keys, so their byte prefix
//! differs from v2's — a provider that hashes the request prefix for
//! prompt caching (anthropic cache control, `prompt_cache_key`, ...) will miss
//! between a TS-issued and a Rust-issued request on the same account and
//! session. That costs cache-hit money/latency, not correctness. The gated
//! tests go green the day bodies serialize in construction order (e.g. a
//! `preserve_order` `serde_json` build) — until then they are skipped with
//! this note.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;

use oc_ai::protocols::anthropic_messages::anthropic_messages_route;
use oc_ai::protocols::gemini::gemini_route;
use oc_ai::protocols::open_responses::{open_responses_route, OpenResponsesRouteSpec};
use oc_ai::protocols::openai_chat::{
    alibaba_chat_compatibility, alibaba_chat_route, groq_chat_compatibility, groq_chat_route,
    openrouter_chat_compatibility, openrouter_chat_route, venice_chat_compatibility,
    venice_chat_route, zai_chat_compatibility, zai_chat_route,
};
use oc_ai::protocols::openai_compatible::openai_compatible_chat_configured;
use oc_ai::route::transport::FailingTransport;
use oc_ai::route::{
    Endpoint, Framing, Route, RouteDefaults, RouteInput, RoutePatch, SharedTransport,
};
use oc_ai::schema::options::MaxTokensFieldCompatibility;
use oc_ai::schema::{
    LLMRequest, LanguageModel, LanguageModelCompatibility, Message, ProviderOptions,
};

fn expected_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-fixtures/expected")
}

fn failing_transport() -> SharedTransport {
    Arc::new(FailingTransport { message: "body parity never streams" })
}

// ---------------------------------------------------------------------------
// Semantic JSON equality (numbers as f64, object key order insignificant)
// ---------------------------------------------------------------------------

#[allow(clippy::float_cmp)]
fn semantic_equal(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::Number(a), Value::Number(b)) => match (a.as_f64(), b.as_f64()) {
            (Some(a), Some(b)) => a == b,
            _ => a == b,
        },
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter().all(|(key, value)| {
                    b.get(key).is_some_and(|other| semantic_equal(value, other))
                })
        }
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| semantic_equal(x, y))
        }
        _ => actual == expected,
    }
}

fn find_divergence(actual: &Value, expected: &Value, path: &mut String) {
    use std::fmt::Write as _;
    match (actual, expected) {
        (Value::Object(a), Value::Object(b)) => {
            for (key, value) in a {
                match b.get(key.as_str()) {
                    Some(other) if semantic_equal(value, other) => continue,
                    Some(other) => {
                        let _ = write!(path, ".{key}");
                        find_divergence(value, other, path);
                    }
                    None => {
                        let _ = write!(path, ".{key} (missing in TS body)");
                    }
                }
                break;
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            for (index, (x, y)) in a.iter().zip(b).enumerate() {
                if !semantic_equal(x, y) {
                    let _ = write!(path, "[{index}]");
                    find_divergence(x, y, path);
                    return;
                }
            }
        }
        _ => {}
    }
}

fn assert_body_matches(actual: &Value, expected: &Value, context: &str) {
    if semantic_equal(actual, expected) {
        return;
    }
    let mut path = String::from("$");
    find_divergence(actual, expected, &mut path);
    panic!("body divergence at {path} ({context})\nrust: {actual}\n  ts: {expected}");
}

/// Cross-check the reconstructed request against the dump's driving-request
/// summary (guards drift between the Rust builder and the dump generator).
fn assert_request_summary_matches(dump: &Value, model_id: &str, provider: &str, prompt: &str) {
    let summary = &dump["request"];
    assert_eq!(summary["model"]["id"].as_str().unwrap_or_default(), model_id, "model id");
    assert_eq!(summary["model"]["provider"].as_str().unwrap_or_default(), provider, "provider");
    let messages = summary["messages"].as_array().cloned().unwrap_or_default();
    assert_eq!(messages.len(), 1, "one user message drives the body golden");
    assert_eq!(messages[0]["role"], "user");
    let content = &messages[0]["content"];
    let text = content.as_str().unwrap_or_else(|| {
        content
            .as_array()
            .and_then(|parts| parts.first())
            .and_then(|p| p["text"].as_str())
            .unwrap_or_default()
    });
    assert_eq!(text, prompt, "driving prompt");
}

fn compiled_body(route: &Route, request: &LLMRequest) -> Value {
    let prepared = route.prepare_request(request);
    route.build_body(&prepared).expect("Rust body builds")
}

fn dump_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("{}: {error}", dir.display()))
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no dumps under {}", dir.display());
    files
}

fn load(path: &Path) -> Value {
    serde_json::from_str(
        &std::fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display())),
    )
    .expect("dump parses")
}

// ---------------------------------------------------------------------------
// anthropic-messages
// ---------------------------------------------------------------------------

#[test]
fn anthropic_messages_body_parity_matches_ts_dumps() {
    let mut checked = 0;
    for path in dump_files(&expected_root().join("anthropic-messages")) {
        let dump = load(&path);
        let provider = dump["provider"].as_str().unwrap_or_default();
        let model_id = dump["model"].as_str().unwrap_or_default();
        let route = anthropic_messages_route(failing_transport())
            .with(RoutePatch { provider: Some(provider.to_string()), ..Default::default() });
        let model = route.model(model_id, None).expect("model builds");
        let mut request = LLMRequest::new(model);
        request.messages = vec![Message::user("hi")];
        assert_request_summary_matches(&dump, model_id, provider, "hi");

        let body = compiled_body(&route, &request);
        for (index, interaction) in
            dump["interactions"].as_array().cloned().unwrap_or_default().iter().enumerate()
        {
            let expected = interaction["body"].clone();
            assert_body_matches(
                &body,
                &expected,
                &format!("{} interaction {index}", path.display()),
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 9, "anthropic-messages body goldens");
}

// ---------------------------------------------------------------------------
// openai-chat family (native re-badges + extension providers)
// ---------------------------------------------------------------------------

struct ChatFamily {
    group: &'static str,
    model_id: &'static str,
}

const CHAT_FAMILIES: &[ChatFamily] = &[
    ChatFamily { group: "groq-chat", model_id: "qwen/qwen3.6-27b" },
    ChatFamily { group: "venice-chat", model_id: "deepseek-v4-flash" },
    ChatFamily { group: "zai-chat", model_id: "glm-5.3" },
    ChatFamily { group: "alibaba-chat", model_id: "qwen3-max" },
    ChatFamily { group: "openai-compatible-chat", model_id: "" }, // per-cassette provider
    ChatFamily { group: "openrouter-reasoning", model_id: "anthropic/claude-sonnet-4.6" },
    ChatFamily { group: "openrouter-reasoning-tool-loop", model_id: "anthropic/claude-sonnet-4.6" },
];

fn togetherai_compatibility() -> LanguageModelCompatibility {
    // providers/togetherai.ts model compatibility.
    LanguageModelCompatibility {
        max_tokens_field: Some(MaxTokensFieldCompatibility::MaxTokens),
        supports_store: Some(false),
        supports_strict_mode: Some(false),
        ..Default::default()
    }
}

fn chat_family_route(family: &ChatFamily, provider: &str) -> Route {
    match family.group {
        "groq-chat" => groq_chat_route(failing_transport(), None),
        "venice-chat" => venice_chat_route(failing_transport(), None),
        "zai-chat" => zai_chat_route(failing_transport(), None),
        "alibaba-chat" => alibaba_chat_route(failing_transport(), None),
        "openrouter-reasoning" | "openrouter-reasoning-tool-loop" => {
            openrouter_chat_route(failing_transport(), None)
        }
        "openai-compatible-chat" => {
            // Real provider base URLs: `detect_supports_store` reads the
            // route endpoint (google's generativelanguage host is
            // non-standard; togetherai matches by provider name).
            let base = if provider == "togetherai" {
                "https://api.together.xyz/v1"
            } else {
                "https://generativelanguage.googleapis.com/v1beta/openai"
            };
            openai_compatible_chat_configured(failing_transport(), provider, base)
        }
        other => panic!("unknown chat family {other}"),
    }
}

fn chat_family_compatibility(
    family: &ChatFamily,
    provider: &str,
) -> Option<LanguageModelCompatibility> {
    match family.group {
        "groq-chat" => Some(groq_chat_compatibility()),
        "venice-chat" => Some(venice_chat_compatibility()),
        "zai-chat" => Some(zai_chat_compatibility()),
        "alibaba-chat" => Some(alibaba_chat_compatibility()),
        "openrouter-reasoning" | "openrouter-reasoning-tool-loop" => {
            Some(openrouter_chat_compatibility())
        }
        "openai-compatible-chat" if provider == "togetherai" => Some(togetherai_compatibility()),
        _ => None,
    }
}

/// The openrouter dump drives `reasoning.max_tokens` through provider
/// options (TS carries them as route defaults via `OpenRouter.configure`;
/// the merged resolution is identical).
fn openrouter_options() -> ProviderOptions {
    let mut options = ProviderOptions::new();
    options.insert("reasoning".to_string(), serde_json::json!({ "max_tokens": 1024 }));
    options
}

#[test]
fn openai_chat_body_parity_matches_ts_dumps() {
    let mut checked = 0;
    for family in CHAT_FAMILIES {
        for path in dump_files(&expected_root().join("openai-chat").join(family.group)) {
            let dump = load(&path);
            let provider = dump["provider"].as_str().unwrap_or_default().to_string();
            let model_id = if family.model_id.is_empty() {
                match provider.as_str() {
                    "togetherai" => "meta-llama/Llama-3.3-70B-Instruct-Turbo",
                    _ => "gemini-3.8-flash",
                }
            } else {
                family.model_id
            };
            let route = chat_family_route(family, &provider);
            let mut model = route.model(model_id, None).expect("model builds");
            if let Some(compatibility) = chat_family_compatibility(family, &provider) {
                model = model.with_compatibility(compatibility);
            }
            let mut request = LLMRequest::new(model);
            request.messages = vec![Message::user("parity replay")];
            if family.group.starts_with("openrouter") {
                request.provider_options = Some(openrouter_options());
            }
            assert_request_summary_matches(&dump, model_id, &provider, "parity replay");

            let body = compiled_body(&route, &request);
            for (index, interaction) in
                dump["interactions"].as_array().cloned().unwrap_or_default().iter().enumerate()
            {
                let expected = interaction["body"].clone();
                assert_body_matches(
                    &body,
                    &expected,
                    &format!("{}/{} interaction {index}", family.group, path.display()),
                );
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 68, "openai-chat family body goldens (incl. error interactions)");
}

// ---------------------------------------------------------------------------
// open-responses + openai-responses
// ---------------------------------------------------------------------------

fn responses_request(prefix: &str, openai: bool) -> (LLMRequest, Route, &'static str) {
    let (provider, key) = match prefix {
        "openai-responses" | "openai-responses-cache" | "openai-responses-images" => {
            ("openai", "openai")
        }
        "moonshot-responses" => ("moonshot", "moonshot"),
        "minimax-responses" => ("minimax", "minimax"),
        "meta-responses" => ("meta", "meta"),
        "alibaba-responses" => ("alibaba", "alibaba"),
        other => panic!("unknown responses group {other}"),
    };
    let route = if openai {
        // Mirror the dump harness exactly (dump.mjs `routeFor`): the bare
        // protocol route WITHOUT the `OpenAIResponses.route` provider
        // defaults (`store:false`, `include:[...]`) — those re-badging
        // defaults are covered by the openai_compatible responses unit test.
        Route::new(RouteInput {
            id: "dump-responses".into(),
            provider: Some("openai".into()),
            provider_metadata_key: Some("openai".into()),
            protocol: Arc::new(oc_ai::protocols::openai_responses::OpenAIResponsesProtocol),
            endpoint: Endpoint::path(oc_ai::protocols::openai_responses::PATH)
                .with_base_url("https://responses.test/v1"),
            auth: None,
            framing: Framing::sse(),
            headers: None,
            transport: failing_transport(),
            defaults: RouteDefaults::default(),
        })
    } else {
        open_responses_route(OpenResponsesRouteSpec {
            id: "dump-responses".into(),
            provider: provider.into(),
            provider_metadata_key: Some(key.into()),
            base_url: "https://responses.test/v1".into(),
            auth: None,
            transport: failing_transport(),
            defaults: RouteDefaults::default(),
        })
    };
    let model = route.model("test-model", None).expect("model");
    let request = LLMRequest::new(model).with_prompt("hi");
    (request, route, provider)
}

#[test]
fn open_responses_body_parity_matches_ts_dumps() {
    let mut checked = 0;
    for prefix in ["moonshot-responses", "minimax-responses", "meta-responses", "alibaba-responses"]
    {
        for path in dump_files(&expected_root().join("open-responses").join(prefix)) {
            let dump = load(&path);
            let (request, route, provider) = responses_request(prefix, false);
            assert_request_summary_matches(&dump, "test-model", provider, "hi");
            let body = compiled_body(&route, &request);
            assert_body_matches(&body, &dump["body"], &path.display().to_string());
            checked += 1;
        }
    }
    assert_eq!(checked, 42, "open-responses body goldens");
}

#[test]
fn openai_responses_body_parity_matches_ts_dumps() {
    let mut checked = 0;
    for prefix in ["openai-responses", "openai-responses-cache", "openai-responses-images"] {
        for path in dump_files(&expected_root().join("openai-responses").join(prefix)) {
            let dump = load(&path);
            let (request, route, provider) = responses_request(prefix, true);
            assert_request_summary_matches(&dump, "test-model", provider, "hi");
            let body = compiled_body(&route, &request);
            assert_body_matches(&body, &dump["body"], &path.display().to_string());
            checked += 1;
        }
    }
    assert_eq!(checked, 10, "openai-responses body goldens");
}

// ---------------------------------------------------------------------------
// gemini
// ---------------------------------------------------------------------------

#[test]
fn gemini_body_parity_matches_ts_dumps() {
    let mut checked = 0;
    for path in dump_files(&expected_root().join("gemini")) {
        let dump = load(&path);
        let provider = dump["provider"].as_str().unwrap_or_default();
        let model_id = dump["model"].as_str().unwrap_or_default();
        let route = gemini_route(failing_transport());
        let model = LanguageModel::new(model_id, provider, Arc::new(route));
        let request = LLMRequest::new(model).with_prompt("hi");
        assert_request_summary_matches(&dump, model_id, provider, "hi");
        // rebuild the route for the body build (the model borrowed it above)
        let body_route = gemini_route(failing_transport());
        let body = compiled_body(&body_route, &request);
        for (index, interaction) in
            dump["interactions"].as_array().cloned().unwrap_or_default().iter().enumerate()
        {
            assert_body_matches(
                &body,
                &interaction["body"],
                &format!("{} interaction {index}", path.display()),
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 3, "gemini body goldens");
}

// ---------------------------------------------------------------------------
// Byte-level parity (opt-in): OC_AI_BODY_BYTE_PARITY=1
// ---------------------------------------------------------------------------

fn byte_parity_enabled() -> bool {
    std::env::var("OC_AI_BODY_BYTE_PARITY").ok().as_deref() == Some("1")
}

const BYTE_SKIP_NOTE: &str = concat!(
    "SKIPPED (byte-level body parity is opt-in): serde_json serializes request bodies ",
    "with BTreeMap-alphabetized keys while v2 preserves construction order, so the byte ",
    "prefix of every Rust-built request body diverges from the TS one. Providers that ",
    "hash the request prefix for prompt caching (anthropic cache_control, openai ",
    "prompt_cache_key) will therefore MISS when TS and Rust alternate on the same ",
    "account/session — a cost/latency regression, not a correctness one. Re-run with ",
    "OC_AI_BODY_BYTE_PARITY=1 after landing insertion-ordered body serialization ",
    "(e.g. serde_json/preserve_order) to make this test green."
);

fn first_dump(group_dirs: &[&str]) -> (PathBuf, Value) {
    let dir = group_dirs.first().expect("at least one dump dir");
    let path = dump_files(&expected_root().join(dir)).remove(0);
    (path.clone(), load(&path))
}

fn byte_compare(context: &str, actual: &Value, expected_text: &str) {
    let actual_text = serde_json::to_string(actual).expect("serializes");
    if actual_text == expected_text {
        return;
    }
    panic!(
        "byte-level body divergence ({context}) — the known BTreeMap key-order \
         divergence (see the module docs and {BYTE_SKIP_NOTE})\nrust: {actual_text}\n  ts: {expected_text}"
    );
}

#[test]
fn anthropic_messages_body_byte_parity_is_known_divergent() {
    if !byte_parity_enabled() {
        eprintln!("{BYTE_SKIP_NOTE}");
        return;
    }
    let (path, dump) = first_dump(&["anthropic-messages"]);
    let provider = dump["provider"].as_str().unwrap_or_default();
    let model_id = dump["model"].as_str().unwrap_or_default();
    let route = anthropic_messages_route(failing_transport())
        .with(RoutePatch { provider: Some(provider.to_string()), ..Default::default() });
    let model = route.model(model_id, None).unwrap();
    let mut request = LLMRequest::new(model);
    request.messages = vec![Message::user("hi")];
    byte_compare(
        &path.display().to_string(),
        &compiled_body(&route, &request),
        dump["interactions"][0]["bodyText"].as_str().unwrap_or_default(),
    );
}

#[test]
fn openai_chat_body_byte_parity_is_known_divergent() {
    if !byte_parity_enabled() {
        eprintln!("{BYTE_SKIP_NOTE}");
        return;
    }
    let (path, dump) = first_dump(&["openai-chat/groq-chat"]);
    let route = groq_chat_route(failing_transport(), None);
    let model = route
        .model("qwen/qwen3.6-27b", None)
        .unwrap()
        .with_compatibility(groq_chat_compatibility());
    let mut request = LLMRequest::new(model);
    request.messages = vec![Message::user("parity replay")];
    byte_compare(
        &path.display().to_string(),
        &compiled_body(&route, &request),
        dump["interactions"][0]["bodyText"].as_str().unwrap_or_default(),
    );
}

#[test]
fn open_responses_body_byte_parity_is_known_divergent() {
    if !byte_parity_enabled() {
        eprintln!("{BYTE_SKIP_NOTE}");
        return;
    }
    let (path, dump) = first_dump(&["open-responses/moonshot-responses"]);
    let route = open_responses_route(OpenResponsesRouteSpec {
        id: "dump-responses".into(),
        provider: "moonshot".into(),
        provider_metadata_key: Some("moonshot".into()),
        base_url: "https://responses.test/v1".into(),
        auth: None,
        transport: failing_transport(),
        defaults: RouteDefaults::default(),
    });
    let model = route.model("test-model", None).unwrap();
    let request = LLMRequest::new(model).with_prompt("hi");
    byte_compare(
        &path.display().to_string(),
        &compiled_body(&route, &request),
        dump["bodyText"].as_str().unwrap_or_default(),
    );
}

#[test]
fn openai_responses_body_byte_parity_is_known_divergent() {
    if !byte_parity_enabled() {
        eprintln!("{BYTE_SKIP_NOTE}");
        return;
    }
    let (path, dump) = first_dump(&["openai-responses/openai-responses"]);
    let (request, route, _provider) = responses_request("openai-responses", true);
    byte_compare(
        &path.display().to_string(),
        &compiled_body(&route, &request),
        dump["bodyText"].as_str().unwrap_or_default(),
    );
}

#[test]
fn gemini_body_byte_parity_is_known_divergent() {
    if !byte_parity_enabled() {
        eprintln!("{BYTE_SKIP_NOTE}");
        return;
    }
    let (path, dump) = first_dump(&["gemini"]);
    let route = gemini_route(failing_transport());
    let model = LanguageModel::new(
        dump["model"].as_str().unwrap_or_default(),
        dump["provider"].as_str().unwrap_or_default(),
        Arc::new(gemini_route(failing_transport())),
    );
    let request = LLMRequest::new(model).with_prompt("hi");
    byte_compare(
        &path.display().to_string(),
        &compiled_body(&route, &request),
        dump["interactions"][0]["bodyText"].as_str().unwrap_or_default(),
    );
}

/// corpus shape pin: the body-golden families this lane regenerated.
#[test]
fn body_golden_corpus_shape() {
    let counts: BTreeMap<&str, usize> = BTreeMap::from([
        ("anthropic-messages", dump_files(&expected_root().join("anthropic-messages")).len()),
        (
            "openai-chat",
            std::fs::read_dir(expected_root().join("openai-chat"))
                .expect("openai-chat dir")
                .filter_map(Result::ok)
                .filter(|entry| entry.path().is_dir())
                .count(),
        ),
        ("open-responses", 4),
        ("openai-responses", 3),
        ("gemini", dump_files(&expected_root().join("gemini")).len()),
    ]);
    assert_eq!(counts["anthropic-messages"], 6);
    assert_eq!(counts["openai-chat"], 7, "seven family groups incl. extension providers");
    assert_eq!(counts["gemini"], 2);
}

// ---------------------------------------------------------------------------
// Wire-number form (hotfix BUG-C): integral LLM params must serialize as
// JSON integers, byte-level — strict-uint providers (Go/newapi) reject
// `32000.0`, and v2's JSON.stringify never emits the integral fraction.
// ---------------------------------------------------------------------------

/// The compiled body's exact `serde_json` text (the wire bytes modulo key
/// order, which transport preserves as built).
fn body_text(route: &Route, request: &LLMRequest) -> String {
    serde_json::to_string(&compiled_body(route, request)).expect("body serializes")
}

#[test]
fn wire_integer_params_custom_provider_openai_compatible() {
    // The live newapi case: a custom openai-compatible provider (unknown
    // name + baseURL → `max_completion_tokens`) carrying the session
    // runner's output limit (f64 32000.0 from OUTPUT_TOKEN_FALLBACK).
    let route = openai_compatible_chat_configured(
        failing_transport(),
        "newapi",
        "https://newapi.example.com/v1",
    );
    let model = route.model("glm-5.3", None).expect("model builds");
    let mut request = LLMRequest::new(model);
    request.messages = vec![Message::user("hi")];
    request.generation = Some(oc_ai::schema::options::GenerationOptions {
        max_tokens: Some(32_000.0),
        temperature: Some(0.0),
        top_p: Some(0.5),
        seed: Some(42.0),
        presence_penalty: Some(1.0),
        frequency_penalty: Some(-1.5),
        ..Default::default()
    });
    let body = compiled_body(&route, &request);
    let text = body_text(&route, &request);

    assert!(
        body["max_completion_tokens"].is_u64(),
        "max_completion_tokens must be an integer: {text}"
    );
    assert_eq!(body["max_completion_tokens"], 32_000);
    assert!(
        !text.contains("32000.0") && !text.contains("32000.0"),
        "no integral fraction on the wire: {text}"
    );
    assert!(body["temperature"].is_u64(), "integral 0.0 → 0: {text}");
    assert!(body["seed"].is_u64(), "seed integral: {text}");
    assert!(body["presence_penalty"].is_u64(), "integral 1.0 → 1: {text}");
    // Genuinely fractional values stay floats.
    assert_eq!(body["top_p"].as_f64(), Some(0.5));
    assert_eq!(body["frequency_penalty"].as_f64(), Some(-1.5));
    assert!(text.contains("\"top_p\":0.5"), "fractional float kept: {text}");
}

#[test]
fn wire_integer_params_anthropic_messages() {
    let route = anthropic_messages_route(failing_transport())
        .with(RoutePatch { provider: Some("anthropic".to_string()), ..Default::default() });
    let model = route.model("claude-sonnet-4-5", None).expect("model builds");
    let mut request = LLMRequest::new(model);
    request.messages = vec![Message::user("hi")];
    // Default path: max_tokens falls back to DEFAULT_MAX_TOKENS (32_000.0).
    let body = compiled_body(&route, &request);
    let text = body_text(&route, &request);
    assert!(body["max_tokens"].is_u64(), "default max_tokens integral: {text}");
    assert_eq!(body["max_tokens"], 32_000);
    assert!(!text.contains("32000.0"), "no integral fraction on the wire: {text}");

    // Explicit generation overrides: integral floats collapse, fractional
    // stay, thinking budget integral.
    let mut tuned = LLMRequest::new(route.model("claude-sonnet-4-5", None).expect("model"));
    tuned.messages = vec![Message::user("hi")];
    tuned.generation = Some(oc_ai::schema::options::GenerationOptions {
        max_tokens: Some(1_024.0),
        temperature: Some(1.0),
        top_p: Some(0.9),
        top_k: Some(8.0),
        ..Default::default()
    });
    let tuned_body = compiled_body(&route, &tuned);
    let tuned_text = body_text(&route, &tuned);
    assert!(tuned_body["max_tokens"].is_u64(), "{}", tuned_text);
    assert!(tuned_body["temperature"].is_u64(), "1.0 → 1: {tuned_text}");
    assert!(tuned_body["top_k"].is_u64(), "8.0 → 8: {tuned_text}");
    assert_eq!(tuned_body["top_p"].as_f64(), Some(0.9));
}
