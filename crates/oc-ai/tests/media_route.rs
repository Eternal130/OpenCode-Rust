//! C7c AI-MEDIA — the media route executor (ledger `AI-M2`).
//!
//! v2's `route/media.ts` owns the transport plumbing every media protocol
//! would otherwise duplicate: auth + deployment headers on submit, query
//! layering, upload-before-submit (`prepare`), absolute provider URLs for
//! fal follow-ups, `READ_RETRY` on status/result (never start/cancel),
//! cancel `activeOnly`, and resume from the encoded token. The rows below
//! drive real queued protocols (`fal-images`, `assemblyai-transcription`,
//! `runway-video`) through the route layer against the cassette-style
//! mock transport, asserting the wire bytes each step produced
//! (`ADR-0006`: no live network).

use std::sync::Arc;
use std::time::Duration;

use oc_ai::media::body::{Query, QueryValue};
use oc_ai::media::image::fal::FalImages;
use oc_ai::media::model::{
    collect_speech_response, CollectedResponse, ImageRequest, SpeechRequest, Status,
    TranscriptionRequest,
};
use oc_ai::media::route::{
    build_url, MediaRouteSpec, QueuedMediaRoute, ReadRetryPolicy, StreamMediaRoute,
};
use oc_ai::media::speech::openai::OpenAISpeech;
use oc_ai::media::transcription::assemblyai::AssemblyAITranscription;
use oc_ai::media::video::runway::RunwayVideo;
use oc_ai::route::auth::{self, Secret};
use oc_ai::test_support::{MockTransport, RecordedInteraction};
use serde_json::{json, Value};

fn test_retry() -> ReadRetryPolicy {
    ReadRetryPolicy {
        max_attempts: 3,
        initial_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(5),
    }
}

fn shared(transport: &Arc<MockTransport>) -> oc_ai::route::transport::SharedTransport {
    Arc::clone(transport) as oc_ai::route::transport::SharedTransport
}

fn spec(transport: &Arc<MockTransport>) -> MediaRouteSpec {
    let mut spec = MediaRouteSpec::new(shared(transport));
    spec.read_retry = test_retry();
    spec.auth = Some(auth::Definition::bearer(Secret::Value("fal-key-1".into())));
    spec
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

fn put(url: &str, status: u16, body: &str) -> RecordedInteraction {
    RecordedInteraction {
        method: "PUT".into(),
        url: url.into(),
        status,
        body: body.into(),
        request_body: None,
    }
}

fn fal_request() -> ImageRequest {
    ImageRequest {
        model: "fal-ai/flux-pro/v1.1".into(),
        prompt: "a red panda".into(),
        ..Default::default()
    }
}

/// The ledger repro row (`crates/oc-ai/tests/media_route.rs::
/// poll_upload_cancel`): a queued route submits through the route's auth
/// and headers, polls the provider-issued absolute URLs (retrying one
/// transient failure), reads the result, cancels via the absolute cancel
/// URL — every step asserting the exact wire request.
#[tokio::test]
async fn poll_upload_cancel() {
    let transport = Arc::new(MockTransport::from_interactions(vec![
        // submit → token with absolute follow-up URLs
        post(
            "https://queue.fal.run/fal-ai/flux-pro/v1.1",
            200,
            r#"{"request_id":"r1","status_url":"https://queue.fal.run/requests/r1/status","response_url":"https://queue.fal.run/requests/r1","cancel_url":"https://queue.fal.run/requests/r1/cancel","queue_position":2}"#,
        ),
        // first status read fails transiently (ProviderInternal → retried)
        get("https://queue.fal.run/requests/r1/status", 500, "{\"error\":\"boom\"}"),
        // retry succeeds
        get("https://queue.fal.run/requests/r1/status", 200, r#"{"status":"IN_PROGRESS"}"#),
        // result read
        get(
            "https://queue.fal.run/requests/r1",
            200,
            r#"{"images":[{"url":"https://cdn.fal.run/img.png","width":1024,"height":768,"content_type":"image/png"}],"seed":42}"#,
        ),
        // cancel
        put("https://queue.fal.run/requests/r1/cancel", 200, "{}"),
    ]));
    let route = QueuedMediaRoute::new(Arc::new(FalImages), spec(&transport));

    let generation = route.start(&fal_request()).await.expect("start submits");
    assert_eq!(generation.first_snapshot().unwrap().status, Status::Queued);
    assert_eq!(generation.first_snapshot().unwrap().position, Some(2.0));

    // The encoded token is the serializable camelCase handle (Generation.token).
    assert_eq!(
        generation.token(),
        &json!({
            "requestId": "r1",
            "statusUrl": "https://queue.fal.run/requests/r1/status",
            "responseUrl": "https://queue.fal.run/requests/r1",
            "cancelUrl": "https://queue.fal.run/requests/r1/cancel",
        })
    );

    // status: the transient 500 is retried, then IN_PROGRESS decodes.
    let snapshot = generation.status().await.expect("status");
    assert_eq!(snapshot.status, Status::Running);

    // result: decodes the images + provider metadata.
    let response = generation.result().await.expect("result");
    assert_eq!(response.images.len(), 1);
    assert_eq!(response.images[0].media_type, "image/png");
    assert_eq!(response.provider_metadata.as_ref().unwrap()["fal"]["requestId"], json!("r1"));

    // cancel: PUT the absolute cancel URL (fal is not activeOnly).
    generation.cancel().await.expect("cancel");

    // Wire assertions, in order.
    let requests = transport.requests();
    assert_eq!(requests.len(), 5, "submit + status(500) + status(200) + result + cancel");
    let submit = &requests[0];
    assert_eq!(submit.method, "POST");
    assert_eq!(submit.url, "https://queue.fal.run/fal-ai/flux-pro/v1.1");
    assert_eq!(submit.header("authorization"), Some("Bearer fal-key-1"));
    assert_eq!(submit.header("content-type"), Some("application/json"));
    let body: Value = serde_json::from_slice(&submit.body).unwrap();
    assert_eq!(body["prompt"], json!("a red panda"));

    let status = &requests[2];
    assert_eq!(status.method, "GET");
    assert_eq!(status.url, "https://queue.fal.run/requests/r1/status");
    assert_eq!(status.header("authorization"), Some("Bearer fal-key-1"));

    let result = &requests[3];
    assert_eq!(result.url, "https://queue.fal.run/requests/r1");
    assert!(result.body.is_empty(), "follow-up calls are bodiless");

    let cancel = &requests[4];
    assert_eq!(cancel.method, "PUT");
    assert_eq!(cancel.url, "https://queue.fal.run/requests/r1/cancel");
}

/// Upload-before-submit (`prepare`): assemblyai uploads inline audio to
/// `/v2/upload` as a binary body through the same route transport, then
/// submits the transcript request with the hosted URL.
#[tokio::test]
async fn upload_before_submit() {
    let transport = Arc::new(MockTransport::from_interactions(vec![
        post(
            "https://api.assemblyai.com/v2/upload",
            200,
            r#"{"upload_url":"https://cdn.assemblyai.com/upload/abc"}"#,
        ),
        post("https://api.assemblyai.com/v2/transcript", 200, r#"{"id":"tr1","status":"queued"}"#),
        get(
            "https://api.assemblyai.com/v2/transcript/tr1",
            200,
            r#"{"id":"tr1","status":"completed","text":"hello world"}"#,
        ),
    ]));
    let mut spec = MediaRouteSpec::new(shared(&transport));
    spec.read_retry = test_retry();
    spec.auth = Some(auth::Definition::bearer(Secret::Value("asm-key".into())));
    let route = QueuedMediaRoute::new(Arc::new(AssemblyAITranscription), spec);

    let request = TranscriptionRequest {
        model: "best".into(),
        audio: oc_ai::schema::media::bytes(vec![1, 2, 3, 4], Some("audio/mpeg".into())),
        ..Default::default()
    };
    let generation = route.start(&request).await.expect("start");
    assert_eq!(generation.token()["transcriptId"], json!("tr1"));
    let response = generation.result().await.expect("result");
    assert_eq!(response.text, "hello world");

    let requests = transport.requests();
    assert_eq!(requests.len(), 3, "upload + submit + result reads the same path");
    let upload = &requests[0];
    assert_eq!(upload.url, "https://api.assemblyai.com/v2/upload");
    assert_eq!(upload.header("content-type"), Some("application/octet-stream"));
    assert_eq!(upload.body.as_ref(), &[1, 2, 3, 4], "the raw audio bytes upload");
    let submit = &requests[1];
    assert_eq!(submit.url, "https://api.assemblyai.com/v2/transcript");
    let body: Value = serde_json::from_slice(&submit.body).unwrap();
    assert_eq!(body["audio_url"], json!("https://cdn.assemblyai.com/upload/abc"));
}

/// `resume` rebuilds a handle from the persisted token (fresh status
/// read); a token the codec rejects fails typed.
#[tokio::test]
async fn resume_round_trips_the_token() {
    let transport = Arc::new(MockTransport::from_interactions(vec![get(
        "https://api.assemblyai.com/v2/transcript/tr9",
        200,
        r#"{"id":"tr9","status":"processing"}"#,
    )]));
    let mut spec = MediaRouteSpec::new(shared(&transport));
    spec.read_retry = test_retry();
    let route = QueuedMediaRoute::new(Arc::new(AssemblyAITranscription), spec);

    let generation = route.resume(&json!({"transcriptId": "tr9"})).await.expect("resume");
    assert_eq!(generation.first_snapshot().unwrap().status, Status::Running);

    // a foreign token shape fails typed at the boundary
    let Err(error) = route.resume(&json!({"requestId": "someone-elses"})).await else {
        panic!("foreign tokens must fail")
    };
    let reason = format!("{:?}", error.reason.tag());
    assert_eq!(reason, "\"InvalidRequest\"");
    assert!(error.message().contains("cannot resume a generation from this token"), "{}", error);
}

/// `activeOnly` cancel: Runway's DELETE destroys finished work, so a
/// terminal status skips the call entirely.
#[tokio::test]
async fn cancel_active_only_skips_terminal_generations() {
    let transport = Arc::new(MockTransport::from_interactions(vec![
        get(
            "https://api.dev.runwayml.com/v1/tasks/t1",
            200,
            r#"{"id":"t1","status":"SUCCEEDED","output":["https://assets/runway1.mp4"]}"#,
        ),
        // activeOnly cancel re-reads the status before deciding
        get(
            "https://api.dev.runwayml.com/v1/tasks/t1",
            200,
            r#"{"id":"t1","status":"SUCCEEDED","output":["https://assets/runway1.mp4"]}"#,
        ),
    ]));
    let mut spec = MediaRouteSpec::new(shared(&transport));
    spec.read_retry = test_retry();
    let route = QueuedMediaRoute::new(Arc::new(RunwayVideo), spec);

    let generation = route.resume(&json!({"taskId": "t1"})).await.expect("resume reads one status");
    assert_eq!(generation.first_snapshot().unwrap().status, Status::Completed);
    generation.cancel().await.expect("cancel skips terminal work");
    let requests = transport.requests();
    assert_eq!(requests.len(), 2, "resume status + cancel's activeOnly status");
    assert!(
        requests.iter().all(|request| request.method == "GET"),
        "no DELETE for a terminal generation"
    );
}

/// Non-2xx submit responses classify through the provider-error taxonomy
/// and never retry (only status/result reads do).
#[tokio::test]
async fn submit_failures_classify_without_retry() {
    let transport = Arc::new(MockTransport::from_interactions(vec![post(
        "https://queue.fal.run/fal-ai/flux-pro/v1.1",
        401,
        r#"{"error":"invalid key"}"#,
    )]));
    let route = QueuedMediaRoute::new(Arc::new(FalImages), spec(&transport));
    let Err(error) = route.start(&fal_request()).await else { panic!("401 submit must fail") };
    assert_eq!(error.reason.tag(), "Authentication", "{error}");
    assert_eq!(transport.requests().len(), 1, "start never retries");
}

/// `READ_RETRY` semantics: exponential gaps capped at 30s, at most 8
/// retries, non-retryable errors stop, and rate limits raise the gap to
/// the provider `retryAfterMs` (route/media.ts:141-165).
#[test]
fn read_retry_policy_matches_v2() {
    let policy = ReadRetryPolicy::default();
    let internal = AiErrorProvider::internal();
    assert_eq!(policy.delay(1, &internal), Some(Duration::from_secs(1)));
    assert_eq!(policy.delay(2, &internal), Some(Duration::from_secs(2)));
    assert_eq!(policy.delay(3, &internal), Some(Duration::from_secs(4)));
    assert_eq!(policy.delay(7, &internal), Some(Duration::from_secs(30)), "capped");
    assert_eq!(policy.delay(8, &internal), Some(Duration::from_secs(30)));
    assert_eq!(policy.delay(9, &internal), None, "recurs(8)");

    // non-retryable errors never retry
    let auth_error = AiErrorProvider::authentication();
    assert_eq!(policy.delay(1, &auth_error), None);

    // rate limits raise the gap to retryAfterMs
    let rate_limited = AiErrorProvider::rate_limit(90_000.0);
    assert_eq!(policy.delay(1, &rate_limited), Some(Duration::from_secs(30)), "raised but capped");
    let small = AiErrorProvider::rate_limit(10_000.0);
    assert_eq!(policy.delay(1, &small), Some(Duration::from_secs(10)));
}

/// URL construction: trailing-slash bases trim, path queries survive,
/// later layers replace per key and repeat array values.
#[test]
fn url_building_layers_queries() {
    let mut one = Query::default();
    one.insert("alt", "sse");
    assert_eq!(
        build_url("https://x.test/v1beta/", "/models/m1:generateContent", &[Some(&one)]),
        "https://x.test/v1beta/models/m1:generateContent?alt=sse"
    );

    // the path's own query survives unless a layer replaces it
    let mut replace = Query::default();
    replace.insert("alt", "json");
    replace.0.insert("k".into(), QueryValue::Many(vec!["a".into(), "b".into()]));
    assert_eq!(
        build_url("https://x.test", "/p?alt=sse&keep=1", &[Some(&replace)]),
        "https://x.test/p?keep=1&alt=json&k=a&k=b"
    );
}

/// The stream route drives the frame machine end-to-end: submit (with the
/// call mode in the body), SSE frames → protocol steps → the terminal
/// finish event, collected by the modality collect fn.
#[tokio::test]
async fn stream_generate_collects_the_finish_event() {
    let transport = Arc::new(MockTransport::from_interactions(vec![
        post("https://api.openai.com/v1/audio/speech", 200, "data: {\"type\":\"speech.audio.delta\",\"audio\":\"AQID\"}\n\ndata: {\"type\":\"speech.audio.done\",\"usage\":{\"input_tokens\":2,\"total_tokens\":3}}\n\n"),
    ]));
    let mut spec = MediaRouteSpec::new(shared(&transport));
    spec.auth = Some(auth::Definition::bearer(Secret::Value("oai".into())));
    let route = StreamMediaRoute::new(Arc::new(OpenAISpeech), spec, collect_speech_response);

    let mut request = SpeechRequest {
        model: "gpt-4o-mini-tts".into(),
        text: "hello".into(),
        ..Default::default()
    };
    request.format = Some("pcm".into());
    let collected = route.generate(request).await.expect("generate collects");
    let CollectedResponse::Speech(response) = collected else { panic!("speech") };
    assert_eq!(response.audio.media_type, "audio/pcm");
    assert_eq!(
        response.audio.info.as_ref().and_then(|info| info.sample_rate),
        Some(24_000.0),
        "pcm finish describes the sample rate"
    );

    let sent = &transport.requests()[0];
    assert_eq!(sent.url, "https://api.openai.com/v1/audio/speech");
    let body: Value = serde_json::from_slice(&sent.body).unwrap();
    assert_eq!(body["input"], json!("hello"));
    assert_eq!(body["response_format"], json!("pcm"));
    assert!(body["stream_format"].is_null(), "generate mode does not opt into SSE");
}

/// A tiny error factory for the policy test (keeps it independent of
/// provider internals).
struct AiErrorProvider;
impl AiErrorProvider {
    fn internal() -> oc_ai::schema::AiError {
        use oc_ai::schema::{AiError, AiErrorReason, ReasonFields};
        AiError::new(AiErrorReason::ProviderInternal {
            fields: ReasonFields::new("transient"),
            retry_after_ms: None,
        })
    }

    fn authentication() -> oc_ai::schema::AiError {
        use oc_ai::schema::{AiError, AiErrorReason, ReasonFields};
        AiError::new(AiErrorReason::Authentication { fields: ReasonFields::new("bad key") })
    }

    fn rate_limit(retry_after_ms: f64) -> oc_ai::schema::AiError {
        use oc_ai::schema::{AiError, AiErrorReason, ReasonFields};
        AiError::new(AiErrorReason::RateLimit {
            fields: ReasonFields::new("slow down"),
            retry_after_ms: Some(retry_after_ms),
            rate_limit: None,
        })
    }
}
