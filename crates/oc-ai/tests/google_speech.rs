//! C7c AI-MEDIA — `google-speech` fixture replay (ledger `AI-M4`).
//!
//! The two v2 recordings under `test-fixtures/recordings/google-speech/`
//! replay through the production media route stack (route executor +
//! protocol + framing) against the cassette mock transport: the unary
//! `generateContent` document and the `streamGenerateContent?alt=sse`
//! stream, asserting the wire URL, the lowered request body, and the
//! collected speech finish event (ADR-0006: no live network).

use std::sync::Arc;

use oc_ai::media::model::{CollectedResponse, SpeechRequest, SpeechVoice};
use oc_ai::media::speech::google::{google_speech_route, GoogleSpeechConfig};
use oc_ai::route::auth::Secret;
use oc_ai::schema::MediaUsage;
use oc_ai::test_support::{Cassette, MockTransport};

fn request(model: &str) -> SpeechRequest {
    SpeechRequest {
        model: model.into(),
        text: "Say: Hello from OpenCode.".into(),
        voice: Some(SpeechVoice::Name("Kore".into())),
        ..Default::default()
    }
}

/// The ledger repro row (`crates/oc-ai/tests/google_speech.rs::
/// fixture_replay`): both cassettes drive the full route stack and the
/// lowered wire matches the recording byte-for-byte (modulo the
/// boundary-free JSON field order).
#[tokio::test]
async fn fixture_replay() {
    // 1. generates-speech: unary generateContent over one document frame.
    let path =
        oc_ai::test_support::recordings_dir().join("google-speech").join("generates-speech.json");
    let cassette = Cassette::load(&path).expect("cassette parses");
    let transport = Arc::new(MockTransport::from_cassette(&cassette));
    let route = google_speech_route(
        Arc::clone(&transport) as oc_ai::route::transport::SharedTransport,
        GoogleSpeechConfig {
            api_key: Some(Secret::Value("fixture".into())),
            base_url: None,
            model_http: None,
        },
    );
    let collected =
        route.generate(request("gemini-2.5-flash-preview-tts")).await.expect("generate");
    let CollectedResponse::Speech(response) = collected else { panic!("speech response") };

    // wire: the recorded URL, x-goog-api-key auth, and the recorded body.
    let sent = &transport.requests()[0];
    assert_eq!(sent.method, "POST");
    assert_eq!(
        sent.url,
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash-preview-tts:generateContent"
    );
    assert_eq!(sent.header("x-goog-api-key"), Some("fixture"));
    let recorded_raw = cassette.interactions[0]
        .request
        .as_ref()
        .and_then(|request| request.body.as_ref())
        .and_then(|body| body.as_str())
        .expect("recording carries the request body");
    let recorded_body: serde_json::Value = serde_json::from_str(recorded_raw).unwrap();
    let sent_body: serde_json::Value = serde_json::from_slice(&sent.body).unwrap();
    assert_eq!(sent_body, recorded_body, "the lowered body matches the recording");

    // finish: L16 PCM at 24 kHz mono, token usage.
    assert!(
        response.audio.media_type.to_lowercase().starts_with("audio/l16"),
        "{}",
        response.audio.media_type
    );
    let info = response.audio.info.as_ref().unwrap();
    assert_eq!(info.encoding.as_deref(), Some("pcm_s16le"));
    assert_eq!(info.sample_rate, Some(24_000.0));
    assert_eq!(info.channels, Some(1.0));
    assert!(matches!(response.usage, Some(MediaUsage::Tokens { .. })), "usage recorded");

    // 2. streams-speech: SSE frames through the same stack.
    let path =
        oc_ai::test_support::recordings_dir().join("google-speech").join("streams-speech.json");
    let cassette = Cassette::load(&path).expect("cassette parses");
    let transport = Arc::new(MockTransport::from_cassette(&cassette));
    let route = google_speech_route(
        Arc::clone(&transport) as oc_ai::route::transport::SharedTransport,
        GoogleSpeechConfig {
            api_key: Some(Secret::Value("fixture".into())),
            base_url: None,
            model_http: None,
        },
    );
    // the stream recording replays in stream mode
    let mut events = route
        .stream(request("gemini-3.1-flash-tts-preview"), oc_ai::media::model::Mode::Stream)
        .await
        .expect("stream submits");
    let mut collected = Vec::new();
    while let Some(event) = futures::StreamExt::next(&mut events).await {
        collected.push(event.expect("stream event"));
    }
    let CollectedResponse::Speech(response) =
        oc_ai::media::model::collect_speech_response(&collected)
    else {
        panic!("speech response")
    };
    let sent = &transport.requests()[0];
    assert_eq!(
        sent.url,
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-3.1-flash-tts-preview:streamGenerateContent?alt=sse"
    );
    assert!(
        response.audio.media_type.to_lowercase().starts_with("audio/l16"),
        "{}",
        response.audio.media_type
    );
    assert_eq!(response.audio.info.as_ref().unwrap().sample_rate, Some(24_000.0));
    assert!(matches!(response.usage, Some(MediaUsage::Tokens { .. })), "usage recorded");
}
