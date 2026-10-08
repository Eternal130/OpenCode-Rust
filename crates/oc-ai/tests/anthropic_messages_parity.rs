//! Anthropic Messages protocol parity against the TS v2 implementation
//! (acceptance gate for this lane).
//!
//! A bun script (`/tmp/opencode/ai-dump/dump-anthropic.ts`, executed against
//! the real v2 sources copied to `/tmp/opencode/ref`) drove the REAL TS step
//! machine (`AnthropicMessages.protocol.stream.step` + the real SSE framing)
//! over every anthropic-messages-protocol cassette's recorded wire bytes and
//! dumped the emitted `LLMEvent` sequences to
//! `test-fixtures/expected/anthropic-messages/<cassette>.json`.
//!
//! This test replays the same cassettes through the Rust route
//! (`anthropic_messages_route` + `MockTransport`, transport-level replay with
//! the raw SSE bodies chunked at 7 bytes) and asserts **exact** equality of
//! the emitted event sequences — values compared as JSON with numbers
//! normalized to `f64` (the only TS/Rust serialization difference: `0` vs
//! `0.0`). A dump may pin a step failure instead; the Rust replay must then
//! fail with the same message and reason tag at the same position.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::StreamExt;
use oc_ai::protocols::anthropic_messages::anthropic_messages_route;
use oc_ai::route::{RoutePatch, SharedTransport};
use oc_ai::test_support::{Cassette, MockTransport};
use serde_json::Value;

fn expected_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("test-fixtures/expected/anthropic-messages")
}

fn recordings_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("test-fixtures/recordings")
}

struct DumpInteraction {
    events: Vec<Value>,
    error: Option<(String, String)>,
}

struct Dump {
    scenario: String,
    provider: String,
    model: String,
    interactions: Vec<DumpInteraction>,
}

fn load_dump(path: &Path) -> Dump {
    let raw = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let value: Value = serde_json::from_str(&raw).expect("dump parses");
    Dump {
        scenario: value["scenario"].as_str().unwrap_or_default().to_string(),
        provider: value["provider"].as_str().unwrap_or_default().to_string(),
        model: value["model"].as_str().unwrap_or_default().to_string(),
        interactions: value["interactions"]
            .as_array()
            .expect("interactions array")
            .iter()
            .map(|interaction| DumpInteraction {
                events: interaction["events"].as_array().cloned().unwrap_or_default(),
                error: interaction["error"].as_object().map(|error| {
                    (
                        error["message"].as_str().unwrap_or_default().to_string(),
                        error["tag"].as_str().unwrap_or_default().to_string(),
                    )
                }),
            })
            .collect(),
    }
}

/// Deep equality over parsed JSON with numbers compared as `f64` (TS dumps
/// `0`, Rust serializes `0.0` — numerically identical).
#[allow(clippy::float_cmp)]
fn json_equal(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::Number(a), Value::Number(b)) => {
            a.as_f64().is_some_and(|a| b.as_f64().is_some_and(|b| a == b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, value)| b.get(key).is_some_and(|other| json_equal(value, other)))
        }
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| json_equal(a, b))
        }
        _ => actual == expected,
    }
}

fn describe_difference(actual: &Value, expected: &Value) -> String {
    match (actual, expected) {
        (Value::Array(a), Value::Array(b)) if a.len() != b.len() => {
            format!("length {} != {}", a.len(), b.len())
        }
        (Value::Array(a), Value::Array(b)) => {
            for (index, (a, b)) in a.iter().zip(b).enumerate() {
                if !json_equal(a, b) {
                    return format!("event #{index}:\n  rust: {a}\n   ts: {b}");
                }
            }
            "arrays equal".into()
        }
        _ => format!("{actual} != {expected}"),
    }
}

#[tokio::test]
async fn anthropic_messages_parity_replays_every_dumped_cassette_exactly() {
    let dumps_dir = expected_dir();
    let mut dump_paths: Vec<PathBuf> = std::fs::read_dir(&dumps_dir)
        .expect("expected/anthropic-messages dump directory exists")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .collect();
    dump_paths.sort();
    assert!(!dump_paths.is_empty(), "no TS dumps found under {}", dumps_dir.display());

    for dump_path in dump_paths {
        let dump = load_dump(&dump_path);
        let cassette_path = recordings_dir().join(format!("{}.json", dump.scenario));
        let cassette = Cassette::load(&cassette_path)
            .unwrap_or_else(|error| panic!("cassette for {}: {error}", dump.scenario));

        let transport: SharedTransport = Arc::new(MockTransport::from_cassette(&cassette));
        // Mirror how each provider's facade wires the route: the provider id
        // (and the providerMetadataKey following it) come from the cassette.
        let route = anthropic_messages_route(Arc::clone(&transport))
            .with(RoutePatch { provider: Some(dump.provider.clone()), ..Default::default() });
        let model =
            route.model(dump.model.clone(), None).expect("model builds for the dumped provider");
        let mut request = oc_ai::schema::LLMRequest::new(model);
        request.messages = vec![oc_ai::schema::Message::user("hi")];

        for (index, interaction) in dump.interactions.iter().enumerate() {
            let mut events: Vec<Value> = Vec::new();
            let mut failure: Option<(String, String)> = None;
            let stream = route.stream(request.clone()).await.expect("stream starts");
            futures::pin_mut!(stream);
            while let Some(item) = stream.next().await {
                match item {
                    Ok(event) => {
                        events.push(serde_json::to_value(&event).expect("event serializes"));
                    }
                    Err(error) => {
                        failure = Some((error.message().to_string(), error.tag().to_string()));
                        break;
                    }
                }
            }
            assert_eq!(
                failure, interaction.error,
                "{}/interaction {index}: error mismatch",
                dump.scenario
            );
            assert!(
                events.len() == interaction.events.len(),
                "{}/interaction {index}: {} events, TS dump has {}: {}",
                dump.scenario,
                events.len(),
                interaction.events.len(),
                describe_difference(
                    &Value::Array(events.clone()),
                    &Value::Array(interaction.events.clone())
                ),
            );
            for (position, (actual, expected)) in
                events.iter().zip(interaction.events.iter()).enumerate()
            {
                assert!(
                    json_equal(actual, expected),
                    "{}/interaction {index} event {position} mismatch:\nrust: {actual}\n ts: {expected}",
                    dump.scenario,
                );
            }
        }
    }
}

/// The dump corpus pins the protocol-lane cassettes: the three recording
/// groups that use the anthropic-messages protocol.
#[test]
fn anthropic_messages_dump_corpus_covers_the_protocol_cassettes() {
    let dumps_dir = expected_dir();
    let mut names: Vec<String> = std::fs::read_dir(&dumps_dir)
        .expect("dumps")
        .flatten()
        .filter_map(|entry| {
            entry.path().file_stem().and_then(|stem| stem.to_str().map(str::to_string))
        })
        .collect();
    names.sort();
    let expected = [
        "anthropic-opus-4-7-image-tool-result",
        "keeps-a-long-tool-turn-inside-the-cache-lookback",
        "minimax-m3-anthropic-compatible-text",
        "minimax-m3-anthropic-compatible-tool-call",
        "minimax-m3-anthropic-compatible-tool-loop",
        "writes-then-reads-cache-control-on-identical-second-call",
    ];
    assert_eq!(names, expected.to_vec());
    // every dump references a cassette that exists
    for name in expected {
        let dump = load_dump(&dumps_dir.join(format!("{name}.json")));
        assert!(Cassette::load(&recordings_dir().join(format!("{}.json", dump.scenario))).is_ok());
        assert_ne!(dump.provider, "");
    }
}

/// The replayed request carries the route's beta headers (the anthropic
/// cassettes recorded with `anthropic-beta` allowed through redaction).
#[tokio::test]
async fn anthropic_messages_replay_sends_beta_and_version_headers() {
    let dump = load_dump(&expected_dir().join("anthropic-opus-4-7-image-tool-result.json"));
    let cassette =
        Cassette::load(&recordings_dir().join(format!("{}.json", dump.scenario))).unwrap();
    let mock = Arc::new(MockTransport::from_cassette(&cassette));
    let route =
        anthropic_messages_route(Arc::clone(&mock) as SharedTransport).with(RoutePatch::default());
    let model = route.model("claude-opus-4-7", None).unwrap();
    let mut request = oc_ai::schema::LLMRequest::new(model);
    request.messages = vec![oc_ai::schema::Message::user("hi")];
    let stream = route.stream(request).await.unwrap();
    futures::pin_mut!(stream);
    while (stream.next().await).is_some() {}

    let requests = mock.requests();
    let headers: BTreeMap<String, String> = requests[0]
        .headers
        .iter()
        .map(|(key, value)| (key.to_ascii_lowercase(), value.clone()))
        .collect();
    assert_eq!(headers.get("anthropic-version").map(String::as_str), Some("2023-06-01"));
    assert_eq!(
        headers.get("anthropic-beta").map(String::as_str),
        Some("interleaved-thinking-2025-05-14")
    );
    let url = requests[0].url.as_str();
    assert!(url.ends_with("/v1/messages?beta=true"), "url: {url}");
}
