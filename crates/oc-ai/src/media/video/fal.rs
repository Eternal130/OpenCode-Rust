//! `fal-video` (port of `protocols/fal-video.ts`). Queued protocol over
//! the shared fal queue template. fal video endpoints are model-specific:
//! last-frame fields are named per model (`end_image_url`,
//! `last_frame_url`, `tail_image_url`), so `frames.last` fails typed and
//! passes through `providerOptions` instead.

use crate::media::body::{merge_records, Body};
use crate::media::fal_queue::{self, FalToken};
use crate::media::model::{
    video_field_present, PollContext, Snapshot, Started, VideoRequest, VideoResponse,
};
use crate::media::protocol::{Identity, MediaProtocolBase, QueuedProtocol, TextOutput};
use crate::schema::{AiError, ProviderMetadata};

pub const ID: &str = "fal-video";

pub fn identity() -> Identity {
    Identity::new(ID, "fal Video", "fal")
}

/// `fromRequest` (fal-video.ts:49-74).
pub fn body_from(request: &VideoRequest) -> Result<Body, AiError> {
    if let Some(frames) = &request.frames {
        if frames.last.is_some() {
            return Err(identity().unsupported(
                "video.frames.last",
                "fal Video names the last frame per model; pass it through providerOptions (e.g. end_image_url) instead of frames.last",
            ));
        }
    }
    let image_url = match &request.frames.as_ref().and_then(|frames| frames.first.as_ref()) {
        Some(first) => Some(fal_queue::media_url(first, "fal Video")?),
        None => None,
    };
    let video_url = match &request.video {
        Some(video) => Some(fal_queue::media_url(video, "fal Video")?),
        None => None,
    };
    let base = crate::media::body::record([
        ("prompt", Some(serde_json::json!(request.prompt))),
        ("negative_prompt", request.negative_prompt.clone().map(|value| serde_json::json!(value))),
        ("seed", request.seed.map(|seed| serde_json::json!(seed))),
        ("aspect_ratio", request.aspect_ratio.clone().map(|value| serde_json::json!(value))),
        ("resolution", request.resolution.clone().map(|value| serde_json::json!(value))),
        ("generate_audio", request.audio.map(|audio| serde_json::json!(audio))),
        ("image_url", image_url.clone().map(|url| serde_json::json!(url))),
        ("video_url", video_url.clone().map(|url| serde_json::json!(url))),
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
    let present: Vec<&str> = ["n", "durationSeconds", "references"]
        .into_iter()
        .filter(|field| video_field_present(request, field))
        .collect();
    identity().reject_unsupported(&present).map_or(Ok(()), Err)
}

/// `decodeResult` (fal-video.ts:82-100).
pub fn decode_result(
    output: &TextOutput,
    context: &PollContext<FalToken>,
) -> Result<VideoResponse, AiError> {
    let value = output.decode_json(output.body)?;
    let video = value.get("video").cloned().unwrap_or(serde_json::Value::Null);
    let url = video.get("url").and_then(serde_json::Value::as_str).unwrap_or_default();
    let media_type = video
        .get("content_type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("video/mp4")
        .to_string();
    let mut fal = std::collections::BTreeMap::new();
    fal.insert("requestId".to_string(), serde_json::json!(context.token.request_id));
    fal.insert(
        "seed".to_string(),
        value.get("seed").filter(|v| !v.is_null()).cloned().unwrap_or(serde_json::Value::Null),
    );
    fal.insert(
        "fileName".to_string(),
        video.get("file_name").filter(|v| !v.is_null()).cloned().unwrap_or(serde_json::Value::Null),
    );
    fal.insert(
        "fileSize".to_string(),
        video.get("file_size").filter(|v| !v.is_null()).cloned().unwrap_or(serde_json::Value::Null),
    );
    if let Some(fields) = value.as_object() {
        for (key, field) in fields {
            if key != "video" && key != "seed" {
                fal.insert(key.clone(), field.clone());
            }
        }
    }
    let mut metadata = ProviderMetadata::new();
    metadata.insert("fal".to_string(), fal);
    Ok(VideoResponse {
        videos: vec![crate::schema::media::url(url, Some(media_type), None)],
        usage: None,
        notices: None,
        provider_metadata: Some(metadata),
    })
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FalVideo;

impl MediaProtocolBase for FalVideo {
    fn identity(&self) -> Identity {
        identity()
    }
    fn unsupported(&self) -> &'static [&'static str] {
        &["n", "durationSeconds", "references"]
    }
    fn base_url(&self) -> String {
        fal_queue::DEFAULT_BASE_URL.to_string()
    }
}

impl QueuedProtocol for FalVideo {
    type Request = VideoRequest;
    type Response = VideoResponse;
    type Token = FalToken;

    fn body_from(&self, request: &VideoRequest) -> Result<Body, AiError> {
        check_unsupported(request)?;
        body_from(request)
    }

    fn start_path(&self, request: &VideoRequest) -> String {
        format!("/{}", request.model)
    }

    fn decode_start(&self, output: &TextOutput) -> Result<Started<FalToken>, AiError> {
        fal_queue::decode_start(output)
    }

    fn status_path(&self, token: &FalToken) -> String {
        token.status_url.clone()
    }

    fn decode_status(
        &self,
        output: &TextOutput,
        context: &PollContext<FalToken>,
    ) -> Result<Snapshot, AiError> {
        fal_queue::decode_status(output, context)
    }

    fn result_path(&self, token: &FalToken) -> String {
        token.response_url.clone()
    }

    fn decode_result(
        &self,
        output: &TextOutput,
        context: &PollContext<FalToken>,
    ) -> Result<VideoResponse, AiError> {
        decode_result(output, context)
    }

    fn cancel(&self) -> Option<crate::media::protocol::CancelSpec> {
        Some(fal_queue::cancel_spec())
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
            model: "fal-ai/veo3".into(),
            prompt: "a sunset".into(),
            ..Default::default()
        }
    }

    #[test]
    fn lowers_the_video_body() {
        let mut video_request = request();
        video_request.negative_prompt = Some("shaky".into());
        video_request.seed = Some(3.0);
        video_request.aspect_ratio = Some("16:9".into());
        video_request.resolution = Some("720p".into());
        video_request.audio = Some(false);
        video_request.frames = Some(crate::media::model::VideoFrames {
            first: Some(media::url("https://x/first.png", None, None)),
            last: None,
        });
        video_request.video = Some(media::url("https://x/src.mp4", None, None));
        let Body::Json { value, .. } = body_from(&video_request).unwrap() else { panic!() };
        assert_eq!(value["prompt"], json!("a sunset"));
        assert_eq!(value["negative_prompt"], json!("shaky"));
        assert_eq!(value["seed"], json!(3));
        assert_eq!(value["aspect_ratio"], json!("16:9"));
        assert_eq!(value["resolution"], json!("720p"));
        assert_eq!(value["generate_audio"], json!(false));
        assert_eq!(value["image_url"], json!("https://x/first.png"));
        assert_eq!(value["video_url"], json!("https://x/src.mp4"));
    }

    #[test]
    fn frames_last_fails_typed() {
        let mut video_request = request();
        video_request.frames = Some(crate::media::model::VideoFrames {
            first: None,
            last: Some(media::url("https://x/last.png", None, None)),
        });
        let error = body_from(&video_request).unwrap_err();
        assert!(matches!(error.reason, crate::schema::AiErrorReason::UnsupportedOperation { .. }));
        assert!(error.message().contains("pass it through providerOptions"));
    }

    #[test]
    fn unsupported_common_fields_reject_before_lowering() {
        let mut video_request = request();
        video_request.duration_seconds = Some(8.0);
        video_request.references = vec![media::url("https://x/r.png", None, None)];
        let error = check_unsupported(&video_request).unwrap_err();
        assert!(error.message().contains("does not support durationSeconds, references"));
    }

    #[test]
    fn decode_result_maps_the_video_payload() {
        let identity = identity();
        let auth = BTreeMap::new();
        let token = FalToken {
            request_id: "req-7".into(),
            status_url: String::new(),
            response_url: String::new(),
            cancel_url: String::new(),
        };
        let context = PollContext { token: &token, auth: &auth };
        let output = TextOutput {
            identity: &identity,
            body: r#"{"video":{"url":"https://v/1.mp4","content_type":"video/mp4","file_name":"1.mp4","file_size":2048},"seed":9,"extra":1}"#,
            http: None,
        };
        let response = decode_result(&output, &context).unwrap();
        assert_eq!(response.video().unwrap().url(), Some("https://v/1.mp4"));
        assert_eq!(response.video().unwrap().media_type, "video/mp4");
        assert_eq!(
            response.provider_metadata.as_ref().unwrap()["fal"]["requestId"],
            json!("req-7")
        );
        assert_eq!(response.provider_metadata.as_ref().unwrap()["fal"]["fileName"], json!("1.mp4"));
        assert_eq!(response.provider_metadata.as_ref().unwrap()["fal"]["extra"], json!(1));
    }
}
