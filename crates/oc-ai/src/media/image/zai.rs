//! `zai-images` (port of `protocols/zai-images.ts`). Inline protocol;
//! Z.ai returns only URLs (no content type) with a 30-day retention, and
//! reports applied content filters alongside a successful result.

use crate::media::body::{merge_records, Body};
use crate::media::model::{image_field_present, DecodeContext, ImageRequest, ImageResponse};
use crate::media::protocol::{Identity, InlineProtocol, MediaProtocolBase, TextOutput};
use crate::schema::media::{Notice, NoticeType};
use crate::schema::AiError;

pub const ID: &str = "zai-images";
pub const DEFAULT_BASE_URL: &str = "https://api.z.ai/api/paas/v4";
pub const PATH: &str = "/images/generations";
/// Z.ai keeps outputs for 30 days (zai-images.ts:11).
pub const OUTPUT_RETENTION_MS: f64 = 30.0 * 24.0 * 60.0 * 60.0 * 1000.0;

pub fn identity() -> Identity {
    Identity::new(ID, "Z.ai Images", "zai")
}

/// `fromRequest` (zai-images.ts:54-62).
pub fn body_from(request: &ImageRequest) -> Result<Body, AiError> {
    let mut native = request.provider_options.clone().unwrap_or_default();
    if let Some(user) = native.remove("userID") {
        native.insert("user_id".to_string(), user);
    }
    let base = crate::media::body::record([
        ("model", Some(serde_json::json!(request.model))),
        ("prompt", Some(serde_json::json!(request.prompt))),
        ("size", request.size.clone().map(|size| serde_json::json!(size))),
    ]);
    let merged = merge_records(&[
        base.as_ref(),
        Some(&native),
        request.http.as_ref().and_then(|h| h.body.as_ref()),
    ])
    .unwrap_or_default();
    Ok(Body::json(merged))
}

pub fn check_unsupported(request: &ImageRequest) -> Result<(), AiError> {
    let present: Vec<&str> = ["images", "mask", "n", "aspectRatio", "seed", "format"]
        .into_iter()
        .filter(|field| image_field_present(request, field))
        .collect();
    identity().reject_unsupported(&present).map_or(Ok(()), Err)
}

/// `decodeResponse` (zai-images.ts:70-100).
pub fn decode(
    output: &TextOutput,
    _context: &DecodeContext<ImageRequest>,
) -> Result<ImageResponse, AiError> {
    let decoded = output.decode_json(output.body)?;
    let data =
        decoded.get("data").and_then(serde_json::Value::as_array).cloned().unwrap_or_default();
    if data.is_empty() {
        return Err(output.invalid("Z.ai Images returned no images"));
    }
    let filters = decoded
        .get("content_filter")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut images = Vec::new();
    for item in &data {
        let url = item.get("url").and_then(serde_json::Value::as_str).unwrap_or_default();
        images.push(crate::media::protocol::expiring_url(url, OUTPUT_RETENTION_MS, None, None));
    }
    let notices: Vec<Notice> = filters
        .iter()
        .map(|filter| {
            let role = filter.get("role").and_then(serde_json::Value::as_str);
            let level = filter.get("level").and_then(serde_json::Value::as_f64);
            let mut zai = std::collections::BTreeMap::new();
            zai.insert("role".to_string(), serde_json::json!(role));
            zai.insert("level".to_string(), serde_json::json!(level));
            let mut metadata = crate::schema::ProviderMetadata::new();
            metadata.insert("zai".to_string(), zai);
            Notice {
                kind: NoticeType::Moderated,
                message: format!(
                    "Z.ai Images applied a content filter{}{}",
                    role.map(|role| format!(" for {role}")).unwrap_or_default(),
                    level.map(|level| format!(" at level {level}")).unwrap_or_default()
                ),
                provider_metadata: Some(metadata),
            }
        })
        .collect();
    let mut zai = std::collections::BTreeMap::new();
    zai.insert("created".to_string(), decoded.get("created").cloned().unwrap_or_default());
    zai.insert("id".to_string(), decoded.get("id").cloned().unwrap_or_default());
    zai.insert("requestID".to_string(), decoded.get("request_id").cloned().unwrap_or_default());
    zai.insert(
        "contentFilter".to_string(),
        decoded.get("content_filter").cloned().unwrap_or_default(),
    );
    let mut metadata = crate::schema::ProviderMetadata::new();
    metadata.insert("zai".to_string(), zai);
    Ok(ImageResponse {
        images,
        notices: if notices.is_empty() { None } else { Some(notices) },
        usage: None,
        provider_metadata: Some(metadata),
    })
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ZAIImages;

impl MediaProtocolBase for ZAIImages {
    fn identity(&self) -> Identity {
        identity()
    }
    fn unsupported(&self) -> &'static [&'static str] {
        &["images", "mask", "n", "aspectRatio", "seed", "format"]
    }
    fn base_url(&self) -> String {
        DEFAULT_BASE_URL.to_string()
    }
}

impl InlineProtocol for ZAIImages {
    type Request = ImageRequest;
    type Response = ImageResponse;

    fn body_from(&self, request: &ImageRequest) -> Result<Body, AiError> {
        check_unsupported(request)?;
        body_from(request)
    }

    fn path(&self, _request: &ImageRequest) -> String {
        PATH.to_string()
    }

    fn decode(
        &self,
        output: &TextOutput,
        context: &DecodeContext<ImageRequest>,
    ) -> Result<ImageResponse, AiError> {
        decode(output, context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request() -> ImageRequest {
        ImageRequest { model: "cogview-4".into(), prompt: "a cat".into(), ..Default::default() }
    }

    #[test]
    fn lowers_model_prompt_and_size() {
        let mut image_request = request();
        image_request.size = Some("1024x1024".into());
        let mut options = crate::schema::ProviderOptions::new();
        options.insert("userID".to_string(), json!("u1"));
        options.insert("quality".to_string(), json!("hd"));
        image_request.provider_options = Some(options);
        let Body::Json { value, .. } = body_from(&image_request).unwrap() else { panic!() };
        assert_eq!(value["model"], json!("cogview-4"));
        assert_eq!(value["size"], json!("1024x1024"));
        assert_eq!(value["user_id"], json!("u1"));
        assert_eq!(value["quality"], json!("hd"));
    }

    #[test]
    fn urls_expire_and_filters_become_notices() {
        let identity = identity();
        let body_text = r#"{"created":1,"id":"img1","data":[{"url":"https://x/1"}],"content_filter":[{"role":"politics","level":2}]}"#;
        let output = TextOutput { identity: &identity, body: body_text, http: None };
        let body = Body::json(Default::default());
        let context = DecodeContext { request: &request(), body: &body };
        let response = decode(&output, &context).unwrap();
        assert_eq!(response.images.len(), 1);
        assert!(response.images[0].expires_at.is_some());
        let notices = response.notices.unwrap();
        assert_eq!(notices[0].kind, NoticeType::Moderated);
        assert_eq!(
            notices[0].message,
            "Z.ai Images applied a content filter for politics at level 2"
        );
    }

    #[test]
    fn no_data_fails_invalid() {
        let identity = identity();
        let output = TextOutput { identity: &identity, body: r#"{"data":[]}"#, http: None };
        let body = Body::json(Default::default());
        let context = DecodeContext { request: &request(), body: &body };
        assert!(decode(&output, &context).unwrap_err().message().contains("no images"));
    }
}
