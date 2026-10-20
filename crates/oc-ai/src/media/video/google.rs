//! `google-video` (port of `protocols/google-video.ts`). Queued protocol
//! over Veo long-running operations. Veo keeps generated files for two
//! days; downloads require the same API key as the poll, so output URLs
//! carry the auth headers transiently.

use std::collections::HashMap;

use crate::media::body::{merge_records, Body};
use crate::media::model::{PollContext, Snapshot, Started, Status, VideoRequest, VideoResponse};
use crate::media::protocol::{
    expiring_url_at, failure, Failure, Identity, MediaProtocolBase, QueuedProtocol, TextOutput,
};
use crate::schema::media::{self, Notice, NoticeType};
use crate::schema::{AiError, ProviderMetadata};

pub const ID: &str = "google-video";
pub const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";
/// Veo keeps generated files for two days (google-video.ts:14).
pub const FILE_RETENTION_MS: f64 = 2.0 * 24.0 * 60.0 * 60.0 * 1000.0;

pub fn identity() -> Identity {
    Identity::new(ID, "Google Veo", "google")
}

/// The serializable token — the long-running operation name
/// (google-video.ts:32).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Token {
    pub operation: String,
}

/// Veo takes inline media only; a prior Veo output is `Media.url` with
/// transient auth, so materialize it first (google-video.ts:80-83).
fn inline_media(route: &str, asset: &media::Asset) -> Result<serde_json::Value, AiError> {
    let inline = crate::media::utils::require_inline(route, asset)?;
    Ok(serde_json::json!({ "inlineData": { "mimeType": inline.mime, "data": inline.base64 } }))
}

/// `fromRequest` (google-video.ts:85-130).
pub fn body_from(request: &VideoRequest) -> Result<Body, AiError> {
    if request.n.is_some_and(|n| n > 1) {
        return Err(identity().unsupported(
            "video.n",
            format!("Google Veo generates one video per request; call it once per video instead of n={}", request.n.unwrap_or_default()),
        ));
    }
    if request.audio == Some(false) {
        return Err(identity().unsupported(
            "video.audio",
            "Google Veo always generates audio; audio: false cannot be honored",
        ));
    }
    if let Some(frames) = &request.frames {
        if frames.last.is_some() && frames.first.is_none() {
            return Err(crate::protocols::shared::invalid_request(
                "Google Veo requires frames.first when frames.last is set",
            ));
        }
    }
    let image = match &request.frames.as_ref().and_then(|frames| frames.first.as_ref()) {
        Some(first) => Some(inline_media("Google Veo", first)?),
        None => None,
    };
    let last_frame = match &request.frames.as_ref().and_then(|frames| frames.last.as_ref()) {
        Some(last) => Some(inline_media("Google Veo", last)?),
        None => None,
    };
    let video = match &request.video {
        Some(video) => Some(inline_media("Google Veo", video)?),
        None => None,
    };
    let mut reference_images = Vec::new();
    for asset in &request.references {
        let image = inline_media("Google Veo", asset)?;
        reference_images.push(serde_json::json!({ "image": image, "referenceType": "asset" }));
    }
    let parameters = merge_records(&[
        crate::media::body::record([
            ("aspectRatio", request.aspect_ratio.clone().map(|value| serde_json::json!(value))),
            ("resolution", request.resolution.clone().map(|value| serde_json::json!(value))),
            ("durationSeconds", request.duration_seconds.map(|value| serde_json::json!(value))),
            (
                "negativePrompt",
                request.negative_prompt.clone().map(|value| serde_json::json!(value)),
            ),
            ("seed", request.seed.map(|seed| serde_json::json!(seed))),
        ])
        .as_ref(),
        request.provider_options.as_ref(),
    ])
    .unwrap_or_default();
    let instances = serde_json::json!([crate::media::body::record([
        ("prompt", Some(serde_json::json!(request.prompt))),
        ("image", image),
        ("lastFrame", last_frame),
        (
            "referenceImages",
            (!reference_images.is_empty()).then(|| serde_json::json!(reference_images))
        ),
        ("video", video),
    ])
    .unwrap_or_default()]);
    let body = serde_json::json!({ "instances": instances, "parameters": parameters });
    let merged = merge_records(&[
        Some(
            serde_json::from_value::<crate::schema::options::JsonRecord>(body).unwrap_or_default(),
        )
        .as_ref(),
        request.http.as_ref().and_then(|http| http.body.as_ref()),
    ])
    .unwrap_or_default();
    Ok(Body::json(merged))
}

/// Operation errors are `google.rpc.Status`; unlisted codes (INTERNAL,
/// UNAVAILABLE, …) are provider-side (google-video.ts:66-73).
fn failure_table() -> HashMap<i64, Failure> {
    HashMap::from([
        (3, Failure::InvalidRequest),  // INVALID_ARGUMENT
        (7, Failure::Authentication),  // PERMISSION_DENIED
        (8, Failure::RateLimit),       // RESOURCE_EXHAUSTED
        (9, Failure::InvalidRequest),  // FAILED_PRECONDITION
        (11, Failure::InvalidRequest), // OUT_OF_RANGE
        (16, Failure::Authentication), // UNAUTHENTICATED
    ])
}

/// Operations carry no status string: not done is running, done with
/// `error` failed, otherwise completed (google-video.ts:142-145).
fn status_of(operation: &serde_json::Value) -> Status {
    if operation.get("done").and_then(serde_json::Value::as_bool) != Some(true) {
        return Status::Running;
    }
    if operation.get("error").is_some() {
        Status::Failed
    } else {
        Status::Completed
    }
}

/// `decodeStart` (google-video.ts:136-139).
pub fn decode_start(output: &TextOutput) -> Result<Started<Token>, AiError> {
    let value = output.decode_json(output.body)?;
    let name =
        value.get("name").and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
    Ok(Started {
        snapshot: Snapshot {
            id: name.clone(),
            status: Status::Running,
            progress: None,
            position: None,
        },
        token: Token { operation: name },
    })
}

/// `decodeStatus` (google-video.ts:149-155).
pub fn decode_status(
    output: &TextOutput,
    context: &PollContext<Token>,
) -> Result<Snapshot, AiError> {
    let operation = output.decode_json(output.body)?;
    Ok(Snapshot {
        id: context.token.operation.clone(),
        status: status_of(&operation),
        progress: None,
        position: None,
    })
}

/// `decodeResult` (google-video.ts:157-205).
pub fn decode_result(
    output: &TextOutput,
    context: &PollContext<Token>,
) -> Result<VideoResponse, AiError> {
    let operation = output.decode_json(output.body)?;
    let status = status_of(&operation);
    if status == Status::Running {
        return Err(output.pending(&context.token.operation));
    }
    if status == Status::Failed {
        let message = operation.pointer("/error/message").and_then(serde_json::Value::as_str);
        let code = operation
            .pointer("/error/code")
            .and_then(serde_json::Value::as_f64)
            .map(|code| code as i64);
        return Err(output.ended(
            Status::Failed,
            format!(
                "Google Veo operation failed{}",
                message.map(|message| format!(": {message}")).unwrap_or_default()
            ),
            failure(&failure_table(), code),
        ));
    }
    let generated = operation.pointer("/response/generateVideoResponse");
    // Downloads require the same API key as the poll; the asset carries it
    // transiently and follows the redirect.
    let mut videos = Vec::new();
    for sample in generated
        .and_then(|value| value.get("generatedSamples"))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        let Some(uri) = sample.pointer("/video/uri").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let media_type = sample
            .pointer("/video/mimeType")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("video/mp4");
        videos.push((uri.to_string(), media_type.to_string()));
    }
    let reasons: Vec<String> = generated
        .and_then(|value| value.get("raiMediaFilteredReasons"))
        .and_then(serde_json::Value::as_array)
        .map(|items| items.iter().filter_map(|item| item.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let filtered_count = generated
        .and_then(|value| value.get("raiMediaFilteredCount"))
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0);
    let notices: Vec<Notice> = reasons
        .iter()
        .map(|reason| {
            let mut google = std::collections::BTreeMap::new();
            google.insert("raiMediaFilteredReason".to_string(), serde_json::json!(reason));
            let mut metadata = ProviderMetadata::new();
            metadata.insert("google".to_string(), google);
            Notice {
                kind: NoticeType::Filtered,
                message: format!("Google Veo filtered media: {reason}"),
                provider_metadata: Some(metadata),
            }
        })
        .collect();
    if videos.is_empty() && (!reasons.is_empty() || filtered_count > 0.0) {
        let suffix =
            if reasons.is_empty() { String::new() } else { format!(": {}", reasons.join("; ")) };
        return Err(output.content_policy(format!("Google Veo filtered every video{suffix}")));
    }
    if videos.is_empty() {
        return Err(output.invalid("Google Veo operation completed without any video"));
    }
    let now = crate::media::protocol::now_millis();
    let mut google = std::collections::BTreeMap::new();
    google.insert("operation".to_string(), serde_json::json!(context.token.operation));
    google.insert("raiMediaFilteredCount".to_string(), serde_json::json!(filtered_count));
    google.insert("metadata".to_string(), operation.get("metadata").cloned().unwrap_or_default());
    let mut metadata = ProviderMetadata::new();
    metadata.insert("google".to_string(), google);
    Ok(VideoResponse {
        videos: videos
            .into_iter()
            .map(|(uri, media_type)| {
                expiring_url_at(uri, now, FILE_RETENTION_MS, Some(media_type), Some(context.auth))
            })
            .collect(),
        usage: None,
        notices: (!notices.is_empty()).then_some(notices),
        provider_metadata: Some(metadata),
    })
}

#[derive(Debug, Clone, Copy, Default)]
pub struct GoogleVideo;

impl MediaProtocolBase for GoogleVideo {
    fn identity(&self) -> Identity {
        identity()
    }
    fn unsupported(&self) -> &'static [&'static str] {
        &[]
    }
    fn base_url(&self) -> String {
        DEFAULT_BASE_URL.to_string()
    }
}

impl QueuedProtocol for GoogleVideo {
    type Request = VideoRequest;
    type Response = VideoResponse;
    type Token = Token;

    fn body_from(&self, request: &VideoRequest) -> Result<Body, AiError> {
        body_from(request)
    }

    fn start_path(&self, request: &VideoRequest) -> String {
        format!("/models/{}:predictLongRunning", request.model)
    }

    fn decode_start(&self, output: &TextOutput) -> Result<Started<Token>, AiError> {
        decode_start(output)
    }

    fn status_path(&self, token: &Token) -> String {
        format!("/{}", token.operation)
    }

    fn decode_status(
        &self,
        output: &TextOutput,
        context: &PollContext<Token>,
    ) -> Result<Snapshot, AiError> {
        decode_status(output, context)
    }

    fn result_path(&self, token: &Token) -> String {
        format!("/{}", token.operation)
    }

    fn decode_result(
        &self,
        output: &TextOutput,
        context: &PollContext<Token>,
    ) -> Result<VideoResponse, AiError> {
        decode_result(output, context)
    }

    fn encode_token(&self, token: &Token) -> Result<serde_json::Value, AiError> {
        serde_json::to_value(token).map_err(|error| {
            identity().frame_error("failed to encode the generation token", error.to_string())
        })
    }

    fn decode_token(&self, raw: &serde_json::Value) -> Result<Token, AiError> {
        serde_json::from_value(raw.clone()).map_err(|error| {
            identity().frame_error("failed to decode the generation token", error.to_string())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::media;
    use serde_json::json;
    use std::collections::BTreeMap;

    fn request() -> VideoRequest {
        VideoRequest {
            model: "veo-3.1-generate-preview".into(),
            prompt: "a sunset".into(),
            ..Default::default()
        }
    }

    #[test]
    fn lowers_instances_and_parameters() {
        let mut video_request = request();
        video_request.aspect_ratio = Some("16:9".into());
        video_request.resolution = Some("1080p".into());
        video_request.duration_seconds = Some(6.0);
        video_request.negative_prompt = Some("shaky".into());
        video_request.frames = Some(crate::media::model::VideoFrames {
            first: Some(media::base64("AQID", "image/png")),
            last: None,
        });
        let Body::Json { value, .. } = body_from(&video_request).unwrap() else { panic!() };
        assert_eq!(
            value["instances"][0],
            json!({
                "prompt": "a sunset",
                "image": { "inlineData": { "mimeType": "image/png", "data": "AQID" } },
            })
        );
        assert_eq!(
            value["parameters"],
            json!({ "aspectRatio": "16:9", "resolution": "1080p", "durationSeconds": 6, "negativePrompt": "shaky" })
        );
    }

    #[test]
    fn frames_last_requires_first_and_audio_cannot_be_disabled() {
        let mut last_only = request();
        last_only.frames = Some(crate::media::model::VideoFrames {
            first: None,
            last: Some(media::base64("AQID", "image/png")),
        });
        let error = body_from(&last_only).unwrap_err();
        assert!(error.message().contains("requires frames.first"));

        let mut muted = request();
        muted.audio = Some(false);
        let error = body_from(&muted).unwrap_err();
        assert!(error.message().contains("always generates audio"));
    }

    #[test]
    fn failure_codes_map_the_rpcs_status() {
        let identity = identity();
        let auth = BTreeMap::new();
        let token = Token { operation: "models/veo/operations/op1".into() };
        let context = PollContext { token: &token, auth: &auth };
        let output = TextOutput {
            identity: &identity,
            body: r#"{"done":true,"error":{"code":8,"message":"quota exceeded"}}"#,
            http: None,
        };
        let error = decode_result(&output, &context).unwrap_err();
        assert!(matches!(error.reason, crate::schema::AiErrorReason::RateLimit { .. }));

        let output = TextOutput {
            identity: &identity,
            body: r#"{"done":true,"error":{"code":13,"message":"internal"}}"#,
            http: None,
        };
        let error = decode_result(&output, &context).unwrap_err();
        assert!(matches!(error.reason, crate::schema::AiErrorReason::ProviderInternal { .. }));
    }

    #[test]
    fn decode_result_attaches_auth_and_notices() {
        let identity = identity();
        let mut auth = BTreeMap::new();
        auth.insert("x-goog-api-key".to_string(), "k".to_string());
        let token = Token { operation: "op1".into() };
        let context = PollContext { token: &token, auth: &auth };
        let output = TextOutput {
            identity: &identity,
            body: r#"{"done":true,"response":{"generateVideoResponse":{"generatedSamples":[{"video":{"uri":"https://v/1.mp4","mimeType":"video/mp4"}}],"raiMediaFilteredCount":1,"raiMediaFilteredReasons":["SAFETY"]}},"metadata":{"p":1}}"#,
            http: None,
        };
        let response = decode_result(&output, &context).unwrap();
        assert_eq!(response.videos.len(), 1);
        assert!(response.videos[0].expires_at.is_some());
        assert_eq!(
            response.videos[0].provider_metadata.as_ref().unwrap()["route"]["headers"]
                ["x-goog-api-key"],
            json!("k")
        );
        assert_eq!(
            response.notices.as_ref().unwrap()[0].message,
            "Google Veo filtered media: SAFETY"
        );
        assert_eq!(
            response.provider_metadata.as_ref().unwrap()["google"]["raiMediaFilteredCount"],
            json!(1.0)
        );
    }

    #[test]
    fn fully_filtered_results_fail_content_policy() {
        let identity = identity();
        let auth = BTreeMap::new();
        let token = Token { operation: "op1".into() };
        let context = PollContext { token: &token, auth: &auth };
        let output = TextOutput {
            identity: &identity,
            body: r#"{"done":true,"response":{"generateVideoResponse":{"raiMediaFilteredCount":2,"raiMediaFilteredReasons":["SAFETY","RECITATION"]}}}"#,
            http: None,
        };
        let error = decode_result(&output, &context).unwrap_err();
        assert!(matches!(error.reason, crate::schema::AiErrorReason::ContentPolicy { .. }));
        assert!(error.message().contains("SAFETY; RECITATION"));
    }

    #[test]
    fn unsupported_fields_reject() {
        let mut video_request = request();
        video_request.n = Some(2);
        let error = body_from(&video_request).unwrap_err();
        assert!(error.message().contains("one video per request"));
    }
}
