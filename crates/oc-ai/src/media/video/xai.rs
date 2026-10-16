//! `xai-video` (port of `protocols/xai-video.ts`). Queued protocol; a
//! source video goes to `/videos/edits` unless `providerOptions.mode`
//! asks for an extension. `progress` is undocumented but observed live as
//! a 0..100 percentage.

use std::collections::BTreeMap;

use crate::media::body::{merge_records, Body};
use crate::media::model::{
    video_field_present, PollContext, Snapshot, Started, Status, VideoRequest, VideoResponse,
};
use crate::media::protocol::{
    failure, status as table_status, Failure, Identity, MediaProtocolBase, QueuedProtocol,
    TextOutput,
};
use crate::schema::media::{self, Notice, NoticeType};
use crate::schema::{AiError, ProviderMetadata};

pub const ID: &str = "xai-video";
pub const DEFAULT_BASE_URL: &str = "https://api.x.ai/v1";
pub const PATH: &str = "/videos/generations";
pub const EDIT_PATH: &str = "/videos/edits";
pub const EXTEND_PATH: &str = "/videos/extensions";
pub const STATUS_PATH: &str = "/videos";

pub fn identity() -> Identity {
    Identity::new(ID, "xAI Video", "xai")
}

/// The serializable token (xai-video.ts:37).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Token {
    pub request_id: String,
}

fn status_table() -> BTreeMap<&'static str, Status> {
    [
        ("pending", Status::Running),
        ("done", Status::Completed),
        ("failed", Status::Failed),
        ("expired", Status::Expired),
    ]
    .into_iter()
    .collect()
}

/// Documented video error codes; `service_unavailable`,
/// `internal_error`, and unknown codes are provider-side
/// (xai-video.ts:69-73).
fn failure_table() -> std::collections::HashMap<&'static str, Failure> {
    [
        ("invalid_argument", Failure::InvalidRequest),
        ("failed_precondition", Failure::InvalidRequest),
        ("permission_denied", Failure::Authentication),
    ]
    .into_iter()
    .collect()
}

/// `mediaInput` (xai-video.ts:79-82): refs become `file_id`, everything
/// else a `url`.
fn media_input(asset: &media::Asset) -> Result<serde_json::Value, AiError> {
    let (kind, value) = crate::media::utils::media_reference(
        asset,
        Some(&crate::schema::ProviderId::new("xai")),
        "xAI Video",
    )?;
    Ok(match kind {
        crate::media::utils::MediaRefKind::Ref => serde_json::json!({ "file_id": value }),
        _ => serde_json::json!({ "url": value }),
    })
}

/// `fromRequest` (xai-video.ts:90-113). `mode` selects the endpoint a
/// `video` input is sent to and does not ride the body.
pub fn body_from(request: &VideoRequest) -> Result<Body, AiError> {
    let image = match &request.frames.as_ref().and_then(|frames| frames.first.as_ref()) {
        Some(first) => Some(media_input(first)?),
        None => None,
    };
    let last_frame = match &request.frames.as_ref().and_then(|frames| frames.last.as_ref()) {
        Some(last) => Some(media_input(last)?),
        None => None,
    };
    let video = match &request.video {
        Some(video) => Some(media_input(video)?),
        None => None,
    };
    let mut references = Vec::new();
    for asset in &request.references {
        references.push(media_input(asset)?);
    }
    let mut native = request.provider_options.clone().unwrap_or_default();
    native.remove("mode");
    let base = crate::media::body::record([
        ("model", Some(serde_json::json!(request.model))),
        ("prompt", Some(serde_json::json!(request.prompt))),
        ("image", image),
        ("last_frame", last_frame),
        ("reference_images", (!references.is_empty()).then(|| serde_json::json!(references))),
        ("video", video),
        ("duration", request.duration_seconds.map(|value| serde_json::json!(value))),
        ("aspect_ratio", request.aspect_ratio.clone().map(|value| serde_json::json!(value))),
        ("resolution", request.resolution.clone().map(|value| serde_json::json!(value))),
        ("generate_audio", request.audio.map(|audio| serde_json::json!(audio))),
    ]);
    let merged = merge_records(&[
        base.as_ref(),
        Some(&native),
        request.http.as_ref().and_then(|http| http.body.as_ref()),
    ])
    .unwrap_or_default();
    Ok(Body::json(merged))
}

pub fn check_unsupported(request: &VideoRequest) -> Result<(), AiError> {
    let present: Vec<&str> = ["n", "seed", "negativePrompt"]
        .into_iter()
        .filter(|field| video_field_present(request, field))
        .collect();
    identity().reject_unsupported(&present).map_or(Ok(()), Err)
}

/// `fraction` (xai-video.ts:125-126): 0..100 → 0..1.
fn fraction(progress: Option<f64>) -> Option<f64> {
    progress.filter(|progress| (0.0..=100.0).contains(progress)).map(|progress| progress / 100.0)
}

/// `decodeStart` (xai-video.ts:119-122).
pub fn decode_start(output: &TextOutput) -> Result<Started<Token>, AiError> {
    let value = output.decode_json(output.body)?;
    let request_id =
        value.get("request_id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    Ok(Started {
        snapshot: Snapshot {
            id: request_id.clone(),
            status: Status::Running,
            progress: None,
            position: None,
        },
        token: Token { request_id },
    })
}

/// `decodeStatus` (xai-video.ts:130-137).
pub fn decode_status(
    output: &TextOutput,
    context: &PollContext<Token>,
) -> Result<Snapshot, AiError> {
    let value = output.decode_json(output.body)?;
    let raw = value.get("status").and_then(|v| v.as_str()).unwrap_or_default();
    Ok(Snapshot {
        id: context.token.request_id.clone(),
        status: table_status(&status_table(), raw, output)?,
        progress: fraction(value.get("progress").and_then(|v| v.as_f64())),
        position: None,
    })
}

/// `decodeResult` (xai-video.ts:139-178). `respect_moderation: false`
/// marks a filtered result; a URL may still be present, so it is reported
/// as a notice.
pub fn decode_result(
    output: &TextOutput,
    context: &PollContext<Token>,
) -> Result<VideoResponse, AiError> {
    let decoded = output.decode_json(output.body)?;
    let raw = decoded.get("status").and_then(|v| v.as_str()).unwrap_or_default();
    let status = table_status(&status_table(), raw, output)?;
    if status == Status::Running {
        return Err(output.pending(&context.token.request_id));
    }
    if status == Status::Failed {
        let code = decoded.pointer("/error/code").and_then(|v| v.as_str());
        let message = decoded.pointer("/error/message").and_then(|v| v.as_str());
        return Err(output.ended(
            Status::Failed,
            format!(
                "xAI Video generation failed{}{}",
                code.map(|code| format!(" ({code})")).unwrap_or_default(),
                message.map(|message| format!(": {message}")).unwrap_or_default()
            ),
            failure(&failure_table(), code),
        ));
    }
    if status != Status::Completed {
        return Err(output.ended(
            Status::Expired,
            format!("xAI Video request {} expired", context.token.request_id),
            Failure::ProviderInternal,
        ));
    }
    let respect_moderation = decoded.pointer("/video/respect_moderation").and_then(|v| v.as_bool());
    let notices = (respect_moderation == Some(false)).then(|| {
        vec![Notice {
            kind: NoticeType::Moderated,
            message: "xAI Video flagged the generated video for moderation".to_string(),
            provider_metadata: None,
        }]
    });
    let url = decoded.pointer("/video/url").and_then(|v| v.as_str());
    if url.is_none() && notices.is_some() {
        return Err(output.content_policy("xAI Video withheld the video for moderation"));
    }
    let Some(url) = url else {
        return Err(output.invalid("xAI Video completed without a video URL"));
    };
    let duration = decoded.pointer("/video/duration").and_then(|v| v.as_f64());
    let mut xai = std::collections::BTreeMap::new();
    xai.insert("requestId".to_string(), serde_json::json!(context.token.request_id));
    xai.insert(
        "model".to_string(),
        decoded.get("model").filter(|v| !v.is_null()).cloned().unwrap_or_default(),
    );
    let mut metadata = ProviderMetadata::new();
    metadata.insert("xai".to_string(), xai);
    let mut video = media::url(url, Some("video/mp4".to_string()), None);
    if let Some(duration) = duration {
        video.info =
            Some(media::Info { duration_seconds: Some(duration), ..media::Info::default() });
    }
    Ok(VideoResponse {
        videos: vec![video],
        usage: None,
        notices,
        provider_metadata: Some(metadata),
    })
}

#[derive(Debug, Clone, Copy, Default)]
pub struct XAIVideo;

impl MediaProtocolBase for XAIVideo {
    fn identity(&self) -> Identity {
        identity()
    }
    fn unsupported(&self) -> &'static [&'static str] {
        &["n", "seed", "negativePrompt"]
    }
    fn base_url(&self) -> String {
        DEFAULT_BASE_URL.to_string()
    }
}

impl QueuedProtocol for XAIVideo {
    type Request = VideoRequest;
    type Response = VideoResponse;
    type Token = Token;

    fn body_from(&self, request: &VideoRequest) -> Result<Body, AiError> {
        check_unsupported(request)?;
        body_from(request)
    }

    fn start_path(&self, request: &VideoRequest) -> String {
        if request.video.is_none() {
            return PATH.to_string();
        }
        match request
            .provider_options
            .as_ref()
            .and_then(|options| options.get("mode"))
            .and_then(|value| value.as_str())
        {
            Some("extend") => EXTEND_PATH.to_string(),
            _ => EDIT_PATH.to_string(),
        }
    }

    fn decode_start(&self, output: &TextOutput) -> Result<Started<Token>, AiError> {
        decode_start(output)
    }

    fn status_path(&self, token: &Token) -> String {
        format!("{STATUS_PATH}/{}", token.request_id)
    }

    fn decode_status(
        &self,
        output: &TextOutput,
        context: &PollContext<Token>,
    ) -> Result<Snapshot, AiError> {
        decode_status(output, context)
    }

    fn result_path(&self, token: &Token) -> String {
        format!("{STATUS_PATH}/{}", token.request_id)
    }

    fn decode_result(
        &self,
        output: &TextOutput,
        context: &PollContext<Token>,
    ) -> Result<VideoResponse, AiError> {
        decode_result(output, context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request() -> VideoRequest {
        VideoRequest {
            model: "grok-video-1".into(),
            prompt: "a sunset".into(),
            ..Default::default()
        }
    }

    #[test]
    fn lowers_generation_body_with_frames_and_refs() {
        let mut video_request = request();
        video_request.frames = Some(crate::media::model::VideoFrames {
            first: Some(media::url("https://x/f.png", None, None)),
            last: None,
        });
        video_request.references = vec![media::reference("xai", "file_1", None)];
        video_request.duration_seconds = Some(6.0);
        video_request.aspect_ratio = Some("16:9".into());
        video_request.resolution = Some("720p".into());
        video_request.audio = Some(true);
        let Body::Json { value, .. } = body_from(&video_request).unwrap() else { panic!() };
        assert_eq!(value["image"], json!({ "url": "https://x/f.png" }));
        assert_eq!(value["reference_images"], json!([{ "file_id": "file_1" }]));
        assert_eq!(value["duration"], json!(6));
        assert_eq!(value["aspect_ratio"], json!("16:9"));
        assert_eq!(value["resolution"], json!("720p"));
        assert_eq!(value["generate_audio"], json!(true));
        assert!(value.get("last_frame").is_none());
    }

    #[test]
    fn mode_selects_extend_and_leaves_the_body() {
        let mut extend = request();
        extend.video = Some(media::url("https://x/src.mp4", None, None));
        let mut options = crate::schema::ProviderOptions::new();
        options.insert("mode".to_string(), json!("extend"));
        extend.provider_options = Some(options.clone());
        assert_eq!(XAIVideo.start_path(&extend), EXTEND_PATH);
        let Body::Json { value, .. } = body_from(&extend).unwrap() else { panic!() };
        assert!(value.get("mode").is_none());

        let mut edit = extend.clone();
        edit.provider_options = Some(options);
        edit.provider_options.as_mut().unwrap().remove("mode");
        assert_eq!(XAIVideo.start_path(&edit), EDIT_PATH);
        assert_eq!(XAIVideo.start_path(&request()), PATH);
    }

    #[test]
    fn progress_normalizes_to_fraction() {
        assert_eq!(fraction(Some(37.0)), Some(0.37));
        assert_eq!(fraction(Some(100.0)), Some(1.0));
        assert_eq!(fraction(Some(150.0)), None);
        assert_eq!(fraction(None), None);
    }

    #[test]
    fn decode_result_maps_moderation_and_duration() {
        let identity = identity();
        let auth = BTreeMap::new();
        let token = Token { request_id: "req-1".into() };
        let context = PollContext { token: &token, auth: &auth };
        let output = TextOutput {
            identity: &identity,
            body: r#"{"status":"done","progress":100,"video":{"url":"https://v/1.mp4","duration":6.5,"respect_moderation":false},"model":"grok-video-1"}"#,
            http: None,
        };
        let response = decode_result(&output, &context).unwrap();
        assert_eq!(response.video().unwrap().url(), Some("https://v/1.mp4"));
        assert_eq!(response.video().unwrap().info.as_ref().unwrap().duration_seconds, Some(6.5));
        assert_eq!(response.notices.as_ref().unwrap()[0].kind, NoticeType::Moderated);

        let withheld = TextOutput {
            identity: &identity,
            body: r#"{"status":"done","video":{"respect_moderation":false}}"#,
            http: None,
        };
        let error = decode_result(&withheld, &context).unwrap_err();
        assert!(matches!(error.reason, crate::schema::AiErrorReason::ContentPolicy { .. }));
    }

    #[test]
    fn error_codes_map_the_documented_table() {
        let identity = identity();
        let auth = BTreeMap::new();
        let token = Token { request_id: "req-1".into() };
        let context = PollContext { token: &token, auth: &auth };
        let denied = TextOutput {
            identity: &identity,
            body: r#"{"status":"failed","error":{"code":"permission_denied","message":"no access"}}"#,
            http: None,
        };
        let error = decode_result(&denied, &context).unwrap_err();
        assert!(matches!(error.reason, crate::schema::AiErrorReason::Authentication { .. }));
        assert!(error.message().contains("(permission_denied): no access"));

        let unknown = TextOutput {
            identity: &identity,
            body: r#"{"status":"failed","error":{"code":"service_unavailable"}}"#,
            http: None,
        };
        let error = decode_result(&unknown, &context).unwrap_err();
        assert!(matches!(error.reason, crate::schema::AiErrorReason::ProviderInternal { .. }));

        let expired =
            TextOutput { identity: &identity, body: r#"{"status":"expired"}"#, http: None };
        let error = decode_result(&expired, &context).unwrap_err();
        assert!(error.message().contains("expired"));
    }
}
