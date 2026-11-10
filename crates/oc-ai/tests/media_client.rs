//! G-347B — the media client facade (`media-client.ts` parity): one
//! `generate`/`stream`/`start`/`resume` surface dispatching on the route
//! kind. Inline and stream lanes fail `start`/`resume` with
//! `UnsupportedOperation`; queued lanes start/resume pollable
//! generations; `stream` expands each modality's response into events.

use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt as _;
use oc_ai::media::client::{ClientEvent, MediaClient, RouteKind, StreamLaneAdapter};
use oc_ai::media::image::google::GoogleImages;
use oc_ai::media::model::{
    CollectedResponse, ImageRequest, ImageResponse, SpeechRequest, TranscriptionRequest,
    TranscriptionResponse,
};
use oc_ai::media::route::InlineMediaRoute;
use oc_ai::media::route::{MediaRouteSpec, QueuedMediaRoute, StreamMediaRoute};
use oc_ai::media::speech::openai::OpenAISpeech;
use oc_ai::media::transcription::assemblyai::AssemblyAITranscription;
use oc_ai::route::auth::{self, Secret};
use oc_ai::schema::AiErrorReason;
use oc_ai::test_support::{MockTransport, RecordedInteraction};

fn shared(transport: &Arc<MockTransport>) -> oc_ai::route::transport::SharedTransport {
    Arc::clone(transport) as oc_ai::route::transport::SharedTransport
}

fn post(url: &str, status: u16, body: &str) -> RecordedInteraction {
    RecordedInteraction {
        method: "POST".into(),
        url: url.into(),
        status,
        body: body.into(),
        request_body: None,
    }
}

fn get(url: &str, status: u16, body: &str) -> RecordedInteraction {
    RecordedInteraction {
        method: "GET".into(),
        url: url.into(),
        status,
        body: body.into(),
        request_body: None,
    }
}

fn image_response_events(response: &ImageResponse) -> Vec<usize> {
    vec![response.images.len()]
}

/// Inline dispatch: `generate` posts once; `stream` expands the response
/// into events; `start`/`resume` fail `UnsupportedOperation` naming the
/// route (media-client.ts:36-49).
#[tokio::test]
async fn inline_lane_dispatches_generate_and_rejects_start() {
    let document = r#"{"candidates":[{"index":0,"content":{"parts":[{"inlineData":{"mimeType":"image/png","data":"AQID"}}]},"finishReason":"STOP"}]}"#;
    let url = "https://generativelanguage.googleapis.com/v1beta/models/imagegen:generateContent";
    let transport = Arc::new(MockTransport::from_interactions(vec![
        post(url, 200, document),
        post(url, 200, document),
    ]));
    let mut spec = MediaRouteSpec::new(shared(&transport));
    spec.auth = Some(auth::Definition::bearer(Secret::Value("g".into())));
    let client = MediaClient::inline(
        "image",
        InlineMediaRoute::new(Arc::new(GoogleImages), spec),
        image_response_events,
    );

    assert_eq!(client.kind(), RouteKind::Inline);

    let request =
        ImageRequest { model: "imagegen".into(), prompt: "a cat".into(), ..Default::default() };
    let response = client.generate(&request).await.expect("inline generate");
    assert_eq!(response.images.len(), 1);

    let mut stream = client.stream(request.clone()).await.expect("inline stream");
    let mut events = Vec::new();
    while let Some(item) = stream.next().await {
        events.push(item.expect("expanded event"));
    }
    assert_eq!(events, vec![ClientEvent::Modality(1)]);

    let (start_error, resume_error) = {
        let Err(start_error) = client.start(&request).await else { panic!("start must fail") };
        let Err(resume_error) = client.resume(&serde_json::json!({})).await else {
            panic!("resume must fail")
        };
        (start_error, resume_error)
    };
    for (operation, error) in [("start", start_error), ("resume", resume_error)] {
        let AiErrorReason::UnsupportedOperation { operation: op, provider, route, fields } =
            error.reason
        else {
            panic!("wrong reason: {error:?}")
        };
        assert_eq!(op, format!("image.{operation}"));
        assert_eq!(provider.map(|p| p.0), Some("google".to_string()));
        assert_eq!(route.as_deref(), Some("google-images"));
        assert_eq!(
            fields.message,
            "google/google-images is not a queued route; use generate or stream"
        );
    }
    // one wire call total: generate for `stream` reused the cassette
    assert_eq!(transport.requests().len(), 2, "generate + stream");
}

/// Queued dispatch: `generate` = start + await; `start` hands back a
/// pollable handle with the serializable token; `resume` rebuilds one
/// (media-client.ts:42-49, 50-53).
#[tokio::test]
async fn queued_lane_starts_resumes_and_awaits() {
    let transport = Arc::new(MockTransport::from_interactions(vec![
        post("https://api.assemblyai.com/v2/transcript", 200, r#"{"id":"tr1","status":"queued"}"#),
        get(
            "https://api.assemblyai.com/v2/transcript/tr1",
            200,
            r#"{"id":"tr1","status":"completed","text":"hello"}"#,
        ),
        get(
            "https://api.assemblyai.com/v2/transcript/tr1",
            200,
            r#"{"id":"tr1","status":"completed","text":"hello"}"#,
        ),
        post("https://api.assemblyai.com/v2/transcript", 200, r#"{"id":"tr1","status":"queued"}"#),
        get(
            "https://api.assemblyai.com/v2/transcript/tr1",
            200,
            r#"{"id":"tr1","status":"processing"}"#,
        ),
    ]));
    let mut spec = MediaRouteSpec::new(shared(&transport));
    spec.auth = Some(auth::Definition::bearer(Secret::Value("asm-key".into())));
    let client = MediaClient::queued(
        "transcription",
        QueuedMediaRoute::new(Arc::new(AssemblyAITranscription), spec),
        |_: &TranscriptionResponse| Vec::<u8>::new(),
    );

    assert_eq!(client.kind(), RouteKind::Queued);

    let request = TranscriptionRequest {
        model: "best".into(),
        audio: oc_ai::schema::media::url(
            "https://cdn.example.com/a.mp3",
            Some("audio/mpeg".into()),
            None,
        ),
        ..Default::default()
    };
    // generate = start + await with default poll options (5s interval —
    // the first status read here is already terminal).
    let response = client.generate(&request).await.expect("queued generate awaits");
    assert_eq!(response.text, "hello");

    // start → handle with token; resume rebuilds a fresh observation.
    let generation = client.start(&request).await.expect("start");
    assert_eq!(generation.token()["transcriptId"], serde_json::json!("tr1"));
    let resumed = client.resume(&generation.token()).await.expect("resume");
    let _ = resumed;
}

/// Stream dispatch: `generate` collects the frame machine; `stream` runs
/// it in stream mode; `start` fails `UnsupportedOperation`
/// (media-client.ts:36-41, 54-55).
#[tokio::test]
async fn stream_lane_collects_and_rejects_start() {
    let transport = Arc::new(MockTransport::from_interactions(vec![post(
        "https://api.openai.com/v1/audio/speech",
        200,
        "data: {\"type\":\"speech.audio.delta\",\"audio\":\"AQID\"}\n\ndata: {\"type\":\"speech.audio.done\",\"usage\":{\"input_tokens\":2,\"total_tokens\":3}}\n\n",
    )]));
    let mut spec = MediaRouteSpec::new(shared(&transport));
    spec.auth = Some(auth::Definition::bearer(Secret::Value("oai".into())));
    let route = Arc::new(StreamMediaRoute::new(
        Arc::new(OpenAISpeech),
        spec,
        oc_ai::media::model::collect_speech_response,
    ));
    #[allow(clippy::result_large_err)] // the adapter seam mirrors the route's Result shape
    let extract = |collected: &CollectedResponse| -> Result<oc_ai::media::model::SpeechResponse, oc_ai::schema::AiError> {
        match collected {
            CollectedResponse::Speech(response) => Ok((**response).clone()),
            _ => unreachable!("speech route collects speech"),
        }
    };
    let client = MediaClient::stream_route(
        "speech",
        StreamLaneAdapter { route: Arc::clone(&route), extract_response: extract },
        |_: &oc_ai::media::model::SpeechResponse| Vec::<oc_ai::media::model::SpeechEvent>::new(),
    );

    assert_eq!(client.kind(), RouteKind::Stream);

    let mut request = SpeechRequest {
        model: "gpt-4o-mini-tts".into(),
        text: "hello".into(),
        ..Default::default()
    };
    request.format = Some("pcm".into());
    let response = client.generate(&request).await.expect("stream generate collects");
    assert_eq!(response.audio.media_type, "audio/pcm");

    let Err(error) = client.start(&request).await else { panic!("start must fail") };
    let AiErrorReason::UnsupportedOperation { operation, fields, .. } = error.reason else {
        panic!("wrong reason: {error:?}")
    };
    assert_eq!(operation, "speech.start");
    assert_eq!(
        fields.message,
        "openai/openai-speech is not a queued route; use generate or stream"
    );
}

/// G-374: the jittered `READ_RETRY` gap stays within Effect's
/// `Schedule.jittered` bounds (0.8×–1.2× of the base gap, capped at the
/// policy max).
#[test]
fn read_retry_jitter_stays_within_bounds() {
    use oc_ai::media::route::ReadRetryPolicy;
    let policy = ReadRetryPolicy {
        max_attempts: 8,
        initial_delay: Duration::from_secs(1),
        max_delay: Duration::from_secs(30),
    };
    let error = oc_ai::schema::AiError::new(AiErrorReason::ProviderInternal {
        fields: oc_ai::schema::ReasonFields::new("transient"),
        retry_after_ms: None,
    });
    let mut distinct = std::collections::HashSet::new();
    for _ in 0..64 {
        let gap = policy.jittered_delay(1, &error).expect("retryable");
        assert!(gap >= Duration::from_millis(799), "below 0.8x: {gap:?}");
        assert!(gap <= Duration::from_millis(1201), "above 1.2x: {gap:?}");
        distinct.insert(gap);
    }
    assert!(distinct.len() > 8, "jitter actually varies: {} distinct", distinct.len());

    // the cap still holds after jitter
    let raised = oc_ai::schema::AiError::new(AiErrorReason::RateLimit {
        fields: oc_ai::schema::ReasonFields::new("slow down"),
        retry_after_ms: Some(90_000.0),
        rate_limit: None,
    });
    let gap = policy.jittered_delay(1, &raised).expect("capped");
    assert!(gap <= Duration::from_secs(30));

    // non-retryable errors never sleep
    let auth_error = oc_ai::schema::AiError::new(AiErrorReason::Authentication {
        fields: oc_ai::schema::ReasonFields::new("bad key"),
    });
    assert_eq!(policy.jittered_delay(1, &auth_error), None);
}

/// G-374: the multipart boundary is random per request — two submits of
/// the same stability form never share a content-type boundary.
#[tokio::test]
async fn multipart_boundary_is_random_per_request() {
    let document = r#"{"image":"AQID","seed":1}"#;
    let transport = Arc::new(MockTransport::from_interactions(vec![
        post("https://api.stability.ai/v2beta/stable-image/generate/ultra", 200, document),
        post("https://api.stability.ai/v2beta/stable-image/generate/ultra", 200, document),
    ]));
    let mut spec = MediaRouteSpec::new(shared(&transport));
    spec.auth = Some(auth::Definition::bearer(Secret::Value("st".into())));
    let route =
        InlineMediaRoute::new(Arc::new(oc_ai::media::image::stability::StabilityImages), spec);

    let request =
        ImageRequest { model: "ultra".into(), prompt: "a cat".into(), ..Default::default() };
    route.generate(&request).await.expect("first generate");
    route.generate(&request).await.expect("second generate");

    let requests = transport.requests();
    let first = requests[0].header("content-type").expect("content-type").to_string();
    let second = requests[1].header("content-type").expect("content-type").to_string();
    assert!(first.starts_with("multipart/form-data; boundary=opencode-media-"), "{first}");
    assert_ne!(first, second, "boundaries must differ per request");
}
