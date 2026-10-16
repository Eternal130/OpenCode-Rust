//! `runway-video` (port of `protocols/runway-video.ts`). Queued protocol;
//! every request must pin the API version. Output URLs are valid 24–48
//! hours; the asset carries the conservative bound.

use std::collections::{BTreeMap, HashMap};

use crate::media::body::{merge_records, Body};
use crate::media::model::{
    video_field_present, PollContext, Snapshot, Started, Status, VideoFrames, VideoRequest,
    VideoResponse,
};
use crate::media::protocol::{
    expiring_url_at, status as table_status, CancelSpec, Failure, Identity, MediaProtocolBase,
    QueuedProtocol, TextOutput,
};
use crate::schema::media;
use crate::schema::{AiError, MediaUsage, ProviderMetadata};

pub const ID: &str = "runway-video";
pub const DEFAULT_BASE_URL: &str = "https://api.dev.runwayml.com/v1";
/// Every Runway request must pin the API version (runway-video.ts:14).
pub const API_VERSION: &str = "2024-11-06";
pub const TEXT_TO_VIDEO_PATH: &str = "/text_to_video";
pub const IMAGE_TO_VIDEO_PATH: &str = "/image_to_video";
pub const VIDEO_TO_VIDEO_PATH: &str = "/video_to_video";
pub const TASKS_PATH: &str = "/tasks";
/// Output URLs are valid for 24–48 hours; the conservative bound
/// (runway-video.ts:20).
pub const OUTPUT_RETENTION_MS: f64 = 24.0 * 60.0 * 60.0 * 1000.0;

pub fn identity() -> Identity {
    Identity::new(ID, "Runway", "runway")
}

/// The serializable token (runway-video.ts:42).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Token {
    pub task_id: String,
}

fn status_table() -> BTreeMap<&'static str, Status> {
    [
        ("PENDING", Status::Queued),
        ("THROTTLED", Status::Queued),
        ("RUNNING", Status::Running),
        ("SUCCEEDED", Status::Completed),
        ("FAILED", Status::Failed),
        ("CANCELLED", Status::Cancelled),
    ]
    .into_iter()
    .collect()
}

/// Runway accepts HTTPS URLs, `runway://` upload URIs, and data URIs, all
/// as one string (runway-video.ts:73-74).
fn media_uri(asset: &media::Asset) -> Result<String, AiError> {
    crate::media::utils::media_reference(
        asset,
        Some(&crate::schema::ProviderId::new("runway")),
        "Runway",
    )
    .map(|(_, value)| value)
}

/// `fromRequest` (runway-video.ts:76-106): `frames` → `promptImage` with
/// positions, `aspectRatio` → `ratio`, `durationSeconds` → `duration`.
pub fn body_from(request: &VideoRequest) -> Result<Body, AiError> {
    let mut prompt_image = Vec::new();
    if let Some(frames) = &request.frames {
        if let Some(first) = &frames.first {
            prompt_image.push(serde_json::json!({ "uri": media_uri(first)?, "position": "first" }));
        }
        if let Some(last) = &frames.last {
            prompt_image.push(serde_json::json!({ "uri": media_uri(last)?, "position": "last" }));
        }
    }
    let video_uri = match &request.video {
        Some(video) => Some(media_uri(video)?),
        None => None,
    };
    let mut references = Vec::new();
    for asset in &request.references {
        references.push(serde_json::json!({ "uri": media_uri(asset)? }));
    }
    let base = crate::media::body::record([
        ("model", Some(serde_json::json!(request.model))),
        ("promptText", Some(serde_json::json!(request.prompt))),
        ("promptImage", (!prompt_image.is_empty()).then(|| serde_json::json!(prompt_image))),
        ("videoUri", video_uri.map(|uri| serde_json::json!(uri))),
        ("references", (!references.is_empty()).then(|| serde_json::json!(references))),
        ("ratio", request.aspect_ratio.clone().map(|value| serde_json::json!(value))),
        ("duration", request.duration_seconds.map(|value| serde_json::json!(value))),
        ("resolution", request.resolution.clone().map(|value| serde_json::json!(value))),
        ("audio", request.audio.map(|audio| serde_json::json!(audio))),
        ("negativePrompt", request.negative_prompt.clone().map(|value| serde_json::json!(value))),
        ("seed", request.seed.map(|seed| serde_json::json!(seed))),
    ]);
    let merged = merge_records(&[
        base.as_ref(),
        request.provider_options.as_ref(),
        request.http.as_ref().and_then(|http| http.body.as_ref()),
    ])
    .unwrap_or_default();
    Ok(Body::json(merged))
}

pub fn check_unsupported(request: &VideoRequest) -> Result<(), AiError> {
    let present: Vec<&str> =
        ["n"].into_iter().filter(|field| video_field_present(request, field)).collect();
    identity().reject_unsupported(&present).map_or(Ok(()), Err)
}

/// `decodeStart` (runway-video.ts:112-115).
pub fn decode_start(output: &TextOutput) -> Result<Started<Token>, AiError> {
    let value = output.decode_json(output.body)?;
    let id = value.get("id").and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
    Ok(Started {
        snapshot: Snapshot {
            id: id.clone(),
            status: Status::Queued,
            progress: None,
            position: None,
        },
        token: Token { task_id: id },
    })
}

/// `decodeStatus` (runway-video.ts:119-126).
pub fn decode_status(
    output: &TextOutput,
    context: &PollContext<Token>,
) -> Result<Snapshot, AiError> {
    let task = output.decode_json(output.body)?;
    let raw = task.get("status").and_then(serde_json::Value::as_str).unwrap_or_default();
    Ok(Snapshot {
        id: context.token.task_id.clone(),
        status: table_status(&status_table(), raw, output)?,
        progress: task.get("progress").and_then(serde_json::Value::as_f64),
        position: None,
    })
}

/// `decodeResult` (runway-video.ts:128-164). Runway failure codes are
/// dotted paths; every moderation outcome carries a SAFETY segment, and
/// `ASSET.INVALID` rejects the caller's input media.
pub fn decode_result(
    output: &TextOutput,
    context: &PollContext<Token>,
) -> Result<VideoResponse, AiError> {
    let task = output.decode_json(output.body)?;
    let raw = task.get("status").and_then(serde_json::Value::as_str).unwrap_or_default();
    let status = table_status(&status_table(), raw, output)?;
    if status == Status::Failed {
        let code = task.get("failureCode").and_then(serde_json::Value::as_str);
        let failure_text = task.get("failure").and_then(serde_json::Value::as_str);
        let message = format!(
            "Runway task failed{}{}",
            code.map(|code| format!(" ({code})")).unwrap_or_default(),
            failure_text.map(|text| format!(": {text}")).unwrap_or_default()
        );
        let Some(code) = code else {
            return Err(output.ended(Status::Failed, message, Failure::ProviderInternal));
        };
        let segments: Vec<&str> = code.split('.').collect();
        if segments.contains(&"SAFETY") {
            return Err(output.content_policy(message));
        }
        let invalid_asset = matches!(segments.first(), Some(first) if *first == "ASSET")
            && matches!(segments.get(1), Some(second) if *second == "INVALID");
        return Err(output.ended(
            Status::Failed,
            message,
            if invalid_asset { Failure::InvalidRequest } else { Failure::ProviderInternal },
        ));
    }
    if status == Status::Cancelled {
        return Err(output.ended(
            Status::Cancelled,
            format!("Runway task {} was cancelled", context.token.task_id),
            Failure::ProviderInternal,
        ));
    }
    if status != Status::Completed {
        return Err(output.pending(&context.token.task_id));
    }
    let urls = task
        .get("output")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items.iter().filter_map(|item| item.as_str().map(str::to_string)).collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if urls.is_empty() {
        return Err(output.invalid("Runway task succeeded without any output"));
    }
    let now = crate::media::protocol::now_millis();
    let mut runway = std::collections::BTreeMap::new();
    runway.insert("taskId".to_string(), serde_json::json!(context.token.task_id));
    runway.insert(
        "estimatedCredits".to_string(),
        task.pointer("/estimatedCost/credits").cloned().unwrap_or_default(),
    );
    let mut metadata = ProviderMetadata::new();
    metadata.insert("runway".to_string(), runway);
    Ok(VideoResponse {
        videos: urls
            .into_iter()
            .map(|url| {
                expiring_url_at(url, now, OUTPUT_RETENTION_MS, Some("video/mp4".to_string()), None)
            })
            .collect(),
        usage: task
            .pointer("/cost/credits")
            .and_then(serde_json::Value::as_f64)
            .map(|credits| MediaUsage::Credits { credits }),
        notices: None,
        provider_metadata: Some(metadata),
    })
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RunwayVideo;

impl MediaProtocolBase for RunwayVideo {
    fn identity(&self) -> Identity {
        identity()
    }
    fn unsupported(&self) -> &'static [&'static str] {
        &["n"]
    }
    fn base_url(&self) -> String {
        DEFAULT_BASE_URL.to_string()
    }
    fn headers(&self) -> Vec<(String, String)> {
        vec![("X-Runway-Version".to_string(), API_VERSION.to_string())]
    }
}

impl QueuedProtocol for RunwayVideo {
    type Request = VideoRequest;
    type Response = VideoResponse;
    type Token = Token;

    fn body_from(&self, request: &VideoRequest) -> Result<Body, AiError> {
        check_unsupported(request)?;
        body_from(request)
    }

    fn start_path(&self, request: &VideoRequest) -> String {
        if request.video.is_some() {
            VIDEO_TO_VIDEO_PATH.to_string()
        } else if request
            .frames
            .as_ref()
            .is_some_and(|frames: &VideoFrames| frames.first.is_some() || frames.last.is_some())
        {
            IMAGE_TO_VIDEO_PATH.to_string()
        } else {
            TEXT_TO_VIDEO_PATH.to_string()
        }
    }

    fn decode_start(&self, output: &TextOutput) -> Result<Started<Token>, AiError> {
        decode_start(output)
    }

    fn status_path(&self, token: &Token) -> String {
        format!("{TASKS_PATH}/{}", token.task_id)
    }

    fn decode_status(
        &self,
        output: &TextOutput,
        context: &PollContext<Token>,
    ) -> Result<Snapshot, AiError> {
        decode_status(output, context)
    }

    fn result_path(&self, token: &Token) -> String {
        format!("{TASKS_PATH}/{}", token.task_id)
    }

    fn decode_result(
        &self,
        output: &TextOutput,
        context: &PollContext<Token>,
    ) -> Result<VideoResponse, AiError> {
        decode_result(output, context)
    }

    fn cancel(&self) -> Option<CancelSpec> {
        // Runway's `DELETE /v1/tasks/{id}` deletes completed tasks and
        // their outputs, so cancel is active-only.
        Some(CancelSpec { method: "DELETE", active_only: true })
    }
}

#[allow(dead_code)]
fn failure_table_unused() -> HashMap<String, Failure> {
    HashMap::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request() -> VideoRequest {
        VideoRequest { model: "gen4_turbo".into(), prompt: "a sunset".into(), ..Default::default() }
    }

    #[test]
    fn text_to_video_lowers_renamed_fields() {
        let mut video_request = request();
        video_request.aspect_ratio = Some("1280:720".into());
        video_request.duration_seconds = Some(5.0);
        video_request.negative_prompt = Some("shaky".into());
        video_request.seed = Some(1.5);
        let Body::Json { value, .. } = body_from(&video_request).unwrap() else { panic!() };
        assert_eq!(value["model"], json!("gen4_turbo"));
        assert_eq!(value["promptText"], json!("a sunset"));
        assert_eq!(value["ratio"], json!("1280:720"));
        assert_eq!(value["duration"], json!(5));
        assert_eq!(value["negativePrompt"], json!("shaky"));
        assert_eq!(value["seed"], json!(1.5));
        assert!(value.get("promptImage").is_none());
    }

    #[test]
    fn frames_become_positioned_prompt_images() {
        let mut video_request = request();
        video_request.frames = Some(VideoFrames {
            first: Some(media::url("https://x/first.png", None, None)),
            last: Some(media::url("https://x/last.png", None, None)),
        });
        video_request.references = vec![media::url("https://x/ref.png", None, None)];
        let Body::Json { value, .. } = body_from(&video_request).unwrap() else { panic!() };
        assert_eq!(
            value["promptImage"],
            json!([
                { "uri": "https://x/first.png", "position": "first" },
                { "uri": "https://x/last.png", "position": "last" }
            ])
        );
        assert_eq!(value["references"], json!([{ "uri": "https://x/ref.png" }]));
    }

    #[test]
    fn start_path_selects_by_input_kind() {
        assert_eq!(RunwayVideo.start_path(&request()), TEXT_TO_VIDEO_PATH);
        let mut framed = request();
        framed.frames = Some(VideoFrames {
            first: Some(media::url("https://x/f.png", None, None)),
            last: None,
        });
        assert_eq!(RunwayVideo.start_path(&framed), IMAGE_TO_VIDEO_PATH);
        let mut sourced = request();
        sourced.video = Some(media::url("https://x/src.mp4", None, None));
        assert_eq!(RunwayVideo.start_path(&sourced), VIDEO_TO_VIDEO_PATH);
    }

    #[test]
    fn decode_result_maps_credits_and_expiry() {
        let identity = identity();
        let auth = BTreeMap::new();
        let token = Token { task_id: "task-1".into() };
        let context = PollContext { token: &token, auth: &auth };
        let output = TextOutput {
            identity: &identity,
            body: r#"{"status":"SUCCEEDED","progress":100,"output":["https://v/1.mp4"],"cost":{"credits":12},"estimatedCost":{"credits":10}}"#,
            http: None,
        };
        let response = decode_result(&output, &context).unwrap();
        assert_eq!(response.videos.len(), 1);
        assert!(response.videos[0].expires_at.is_some());
        assert_eq!(response.usage, Some(MediaUsage::Credits { credits: 12.0 }));
        assert_eq!(
            response.provider_metadata.as_ref().unwrap()["runway"]["estimatedCredits"],
            json!(10)
        );
    }

    #[test]
    fn safety_codes_fail_content_policy_and_asset_invalid_maps_invalid_request() {
        let identity = identity();
        let auth = BTreeMap::new();
        let token = Token { task_id: "t".into() };
        let context = PollContext { token: &token, auth: &auth };
        let safety = TextOutput {
            identity: &identity,
            body: r#"{"status":"FAILED","failureCode":"OUTPUT.SAFETY.VIDEO","failure":"flagged"}"#,
            http: None,
        };
        let error = decode_result(&safety, &context).unwrap_err();
        assert!(matches!(error.reason, crate::schema::AiErrorReason::ContentPolicy { .. }));

        let invalid = TextOutput {
            identity: &identity,
            body: r#"{"status":"FAILED","failureCode":"ASSET.INVALID.FORMAT"}"#,
            http: None,
        };
        let error = decode_result(&invalid, &context).unwrap_err();
        assert!(matches!(error.reason, crate::schema::AiErrorReason::InvalidRequest { .. }));

        let internal = TextOutput {
            identity: &identity,
            body: r#"{"status":"FAILED","failureCode":"ENGINE.CRASHED"}"#,
            http: None,
        };
        let error = decode_result(&internal, &context).unwrap_err();
        assert!(matches!(error.reason, crate::schema::AiErrorReason::ProviderInternal { .. }));
    }

    #[test]
    fn cancel_is_delete_and_active_only() {
        let cancel = RunwayVideo.cancel().unwrap();
        assert_eq!(cancel.method, "DELETE");
        assert!(cancel.active_only);
        assert_eq!(
            RunwayVideo.headers(),
            vec![("X-Runway-Version".to_string(), API_VERSION.to_string())]
        );
    }
}
