//! `fal-images` (port of `protocols/fal-images.ts`). Queued protocol over
//! the shared fal queue template; `/edit` and `/multi` endpoints take an
//! `image_urls` list while the rest take one `image_url`.

use std::collections::BTreeMap;

use crate::media::body::{merge_records, Body};
use crate::media::fal_queue::{self, FalToken};
use crate::media::model::{image_field_present, ImageRequest, ImageResponse, PollContext, Snapshot, Started};
use crate::media::protocol::{Identity, MediaProtocolBase, QueuedProtocol, TextOutput};
use crate::media::utils::dimensions;
use crate::schema::media::{self, Notice, NoticeType};
use crate::schema::{AiError, ProviderMetadata};

pub const ID: &str = "fal-images";

pub fn identity() -> Identity {
    Identity::new(ID, "fal Images", "fal")
}

/// `sizing(model)` (fal-images.ts:51-55).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sizing {
    AspectRatio,
    ImageSize,
}

#[must_use]
pub fn sizing(model: &str) -> Option<Sizing> {
    if model.starts_with("fal-ai/nano-banana")
        || model.starts_with("fal-ai/flux-pro/v1.1-ultra")
        || model.starts_with("fal-ai/flux-pro/kontext")
    {
        return Some(Sizing::AspectRatio);
    }
    if model.starts_with("fal-ai/flux") {
        return Some(Sizing::ImageSize);
    }
    None
}

/// `/edit` and `/multi` (Kontext) endpoints take an `image_urls` list;
/// image-to-image, fill, and Ultra take one `image_url` beside `mask_url`
/// (fal-images.ts:78).
#[must_use]
pub fn takes_image_list(model: &str) -> bool {
    model.ends_with("/edit") || model.ends_with("/multi")
}

/// `validate` (fal-images.ts:57-74).
pub fn validate(request: &ImageRequest) -> Result<(), AiError> {
    let id = &request.model;
    let field = sizing(id);
    if request.size.is_some() && request.aspect_ratio.is_some() {
        return Err(crate::protocols::shared::invalid_request(
            "fal Images accepts either size or aspectRatio, not both",
        ));
    }
    if request.size.is_some() && field == Some(Sizing::AspectRatio) {
        return Err(identity().unsupported("media.size", format!("{id} sizes by aspectRatio")));
    }
    if request.aspect_ratio.is_some() && field == Some(Sizing::ImageSize) {
        return Err(identity().unsupported("media.aspectRatio", format!("{id} sizes by size (image_size)")));
    }
    if request.images.len() > 1 && !takes_image_list(id) {
        return Err(identity().unsupported(
            "media.images",
            format!("{id} takes one image_url; use an /edit or /multi endpoint for several images"),
        ));
    }
    Ok(())
}

/// `fromRequest` (fal-images.ts:80-101).
pub fn body_from(request: &ImageRequest) -> Result<Body, AiError> {
    validate(request)?;
    let mut images = Vec::new();
    for image in &request.images {
        images.push(fal_queue::media_url(image, "fal Images")?);
    }
    let list = takes_image_list(&request.model);
    let (width, height) = request.size.as_deref().map(dimensions).unwrap_or((None, None));
    let image_size = match (width, height) {
        (Some(width), Some(height)) => Some(serde_json::json!({ "width": width as u32, "height": height as u32 })),
        _ => None,
    };
    let base = crate::media::body::record([
        ("prompt", Some(serde_json::json!(request.prompt))),
        ("num_images", request.n.map(|n| serde_json::json!(n))),
        ("seed", request.seed.map(|seed| serde_json::json!(seed))),
        ("image_size", image_size),
        ("aspect_ratio", request.aspect_ratio.clone().map(|ratio| serde_json::json!(ratio))),
        ("output_format", request.format.clone().map(|format| serde_json::json!(format))),
        ("image_urls", (list && !images.is_empty()).then(|| serde_json::json!(images))),
        ("image_url", (!list).then(|| images.first().cloned()).flatten().map(|url| serde_json::json!(url))),
        (
            "mask_url",
            match &request.mask {
                Some(mask) => Some(serde_json::json!(fal_queue::media_url(mask, "fal Images")?)),
                None => None,
            },
        ),
    ]);
    let merged = merge_records(&[
        base.as_ref(),
        request.provider_options.as_ref(),
        request.http.as_ref().and_then(|http| http.body.as_ref()),
    ])
    .unwrap_or_default();
    Ok(Body::json(merged))
}

/// `decodeResult` (fal-images.ts:109-136). With the safety checker on,
/// flagged images come back blacked out rather than omitted.
pub fn decode_result(output: &TextOutput, context: &PollContext<FalToken>) -> Result<ImageResponse, AiError> {
    let value = output.decode_json(output.body)?;
    let images = value.get("images").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    if images.is_empty() {
        return Err(output.invalid("fal Images returned no images"));
    }
    let seed = value.get("seed").filter(|v| !v.is_null()).cloned();
    let nsfw = value
        .get("has_nsfw_concepts")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let flagged: Vec<usize> = nsfw
        .iter()
        .enumerate()
        .filter_map(|(index, flag)| flag.as_bool().unwrap_or(false).then_some(index))
        .collect();
    let mut assets = Vec::new();
    for image in &images {
        let url = image.get("url").and_then(|v| v.as_str()).unwrap_or_default();
        let width = image.get("width").and_then(|v| v.as_f64());
        let height = image.get("height").and_then(|v| v.as_f64());
        let media_type = image.get("content_type").and_then(|v| v.as_str()).map(str::to_string);
        let info = media::Info { width, height, ..media::Info::default() };
        // `sync_mode: true` returns data URIs instead of hosted URLs.
        assets.push(media::parse_data_url(url).unwrap_or_else(|| media::url(url, media_type, None)).tap_info(info));
    }
    let mut fal = std::collections::BTreeMap::new();
    fal.insert("requestId".to_string(), serde_json::json!(context.token.request_id));
    fal.insert("seed".to_string(), seed.clone().unwrap_or(serde_json::Value::Null));
    if let Some(fields) = value.as_object() {
        for (key, field) in fields {
            if key != "images" && key != "seed" && key != "has_nsfw_concepts" {
                fal.insert(key.clone(), field.clone());
            }
        }
    }
    let mut metadata = ProviderMetadata::new();
    metadata.insert("fal".to_string(), fal);
    Ok(ImageResponse {
        images: assets,
        notices: if flagged.is_empty() {
            None
        } else {
            Some(
                flagged
                    .into_iter()
                    .map(|index| Notice {
                        kind: NoticeType::Moderated,
                        message: format!("fal Images flagged image {index} as NSFW"),
                        provider_metadata: None,
                    })
                    .collect(),
            )
        },
        usage: None,
        provider_metadata: Some(metadata),
    })
}

/// Attach info to a parsed data-URL asset (the TS parse carries options).
trait TapInfo {
    fn tap_info(self, info: media::Info) -> media::Asset;
}

impl TapInfo for media::Asset {
    fn tap_info(mut self, info: media::Info) -> media::Asset {
        self.info = Some(info);
        self
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FalImages;

impl MediaProtocolBase for FalImages {
    fn identity(&self) -> Identity {
        identity()
    }
    fn unsupported(&self) -> &'static [&'static str] {
        &[]
    }
    fn base_url(&self) -> String {
        fal_queue::DEFAULT_BASE_URL.to_string()
    }
}

impl QueuedProtocol for FalImages {
    type Request = ImageRequest;
    type Response = ImageResponse;
    type Token = FalToken;

    fn body_from(&self, request: &ImageRequest) -> Result<Body, AiError> {
        body_from(request)
    }

    fn start_path(&self, request: &ImageRequest) -> String {
        format!("/{}", request.model)
    }

    fn decode_start(&self, output: &TextOutput) -> Result<Started<FalToken>, AiError> {
        fal_queue::decode_start(output)
    }

    fn status_path(&self, token: &FalToken) -> String {
        token.status_url.clone()
    }

    fn decode_status(&self, output: &TextOutput, context: &PollContext<FalToken>) -> Result<Snapshot, AiError> {
        fal_queue::decode_status(output, context)
    }

    fn result_path(&self, token: &FalToken) -> String {
        token.response_url.clone()
    }

    fn decode_result(&self, output: &TextOutput, context: &PollContext<FalToken>) -> Result<ImageResponse, AiError> {
        decode_result(output, context)
    }

    fn cancel(&self) -> Option<crate::media::protocol::CancelSpec> {
        Some(fal_queue::cancel_spec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request(model: &str) -> ImageRequest {
        ImageRequest { model: model.into(), prompt: "a cat".into(), ..Default::default() }
    }

    #[test]
    fn flux_sizes_by_image_size() {
        let mut image_request = request("fal-ai/flux/dev");
        image_request.size = Some("768x512".into());
        let Body::Json { value, .. } = body_from(&image_request).unwrap() else { panic!() };
        assert_eq!(value["image_size"], json!({ "width": 768, "height": 512 }));
    }

    #[test]
    fn kontext_edits_take_image_urls_lists() {
        let mut image_request = request("fal-ai/flux-pro/kontext/edit");
        image_request.images = vec![
            media::url("https://x/1.png", None, None),
            media::url("https://x/2.png", None, None),
        ];
        let Body::Json { value, .. } = body_from(&image_request).unwrap() else { panic!() };
        assert_eq!(value["image_urls"], json!(["https://x/1.png", "https://x/2.png"]));
        assert!(value.get("image_url").is_none());
    }

    #[test]
    fn non_list_models_take_one_image_url() {
        let mut image_request = request("fal-ai/flux/dev");
        image_request.images = vec![media::url("https://x/1.png", None, None)];
        let Body::Json { value, .. } = body_from(&image_request).unwrap() else { panic!() };
        assert_eq!(value["image_url"], json!("https://x/1.png"));
        assert!(value.get("image_urls").is_none());
    }

    #[test]
    fn conflicting_sizing_fails_typed() {
        let mut image_request = request("fal-ai/flux/dev");
        image_request.size = Some("512x512".into());
        image_request.aspect_ratio = Some("1:1".into());
        let error = body_from(&image_request).unwrap_err();
        assert!(error.message().contains("either size or aspectRatio, not both"));

        let mut nano = request("fal-ai/nano-banana");
        nano.size = Some("512x512".into());
        let error = body_from(&nano).unwrap_err();
        assert!(error.message().contains("sizes by aspectRatio"));
    }

    #[test]
    fn decode_result_flags_nsfw_and_keeps_extras() {
        let identity = identity();
        let auth = BTreeMap::new();
        let token = FalToken {
            request_id: "req1".into(),
            status_url: String::new(),
            response_url: String::new(),
            cancel_url: String::new(),
        };
        let context = PollContext { token: &token, auth: &auth };
        let output = TextOutput {
            identity: &identity,
            body: r#"{"images":[{"url":"https://x/1.png","width":512,"height":512,"content_type":"image/png"}],"seed":7,"has_nsfw_concepts":[false,true],"extra":"kept"}"#,
            http: None,
        };
        let response = decode_result(&output, &context).unwrap();
        assert_eq!(response.images[0].url(), Some("https://x/1.png"));
        assert_eq!(response.images[0].info.as_ref().unwrap().width, Some(512.0));
        assert_eq!(response.notices.as_ref().unwrap().len(), 1);
        assert_eq!(response.notices.as_ref().unwrap()[0].message, "fal Images flagged image 1 as NSFW");
        assert_eq!(response.provider_metadata.as_ref().unwrap()["fal"]["requestId"], json!("req1"));
        assert_eq!(response.provider_metadata.as_ref().unwrap()["fal"]["seed"], json!(7));
        assert_eq!(response.provider_metadata.as_ref().unwrap()["fal"]["extra"], json!("kept"));
    }
}
