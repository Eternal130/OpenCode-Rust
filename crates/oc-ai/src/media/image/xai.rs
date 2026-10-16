//! `xai-images` (port of `protocols/xai-images.ts`). Inline protocol;
//! xAI reports a USD cost rather than tokens/seconds/credits, so the raw
//! usage record stays in provider metadata only.

use crate::media::body::{merge_records, Body};
use crate::media::model::{image_field_present, DecodeContext, ImageRequest, ImageResponse};
use crate::media::protocol::{Identity, InlineProtocol, MediaProtocolBase, TextOutput};
use crate::media::utils::{image_output, media_reference, MediaRefKind};
use crate::schema::media;
use crate::schema::{AiError, ProviderId};

pub const ID: &str = "xai-images";
pub const DEFAULT_BASE_URL: &str = "https://api.x.ai/v1";
pub const PATH: &str = "/images/generations";
pub const EDIT_PATH: &str = "/images/edits";

pub fn identity() -> Identity {
    Identity::new(ID, "xAI Images", "xai")
}

fn reference(asset: &media::Asset) -> Result<serde_json::Value, AiError> {
    let (kind, value) = media_reference(asset, Some(&ProviderId::new("xai")), "xAI Images")?;
    Ok(match kind {
        MediaRefKind::Ref => serde_json::json!({ "file_id": value }),
        _ => serde_json::json!({ "url": value, "type": "image_url" }),
    })
}

/// `fromRequest` (xai-images.ts:64-81): xAI takes one edit source as
/// `image` and several as `images`.
pub fn body_from(request: &ImageRequest) -> Result<Body, AiError> {
    let mut references = Vec::new();
    for image in &request.images {
        references.push(reference(image)?);
    }
    let mut native = request.provider_options.clone().unwrap_or_default();
    if let Some(format) = native.remove("responseFormat") {
        native.insert("response_format".to_string(), format);
    }
    let base = crate::media::body::record([
        ("model", Some(serde_json::json!(request.model))),
        ("prompt", Some(serde_json::json!(request.prompt))),
        ("image", (references.len() == 1).then(|| references[0].clone())),
        ("images", (references.len() > 1).then(|| serde_json::json!(references))),
        ("n", request.n.map(|n| serde_json::json!(n))),
        ("aspect_ratio", request.aspect_ratio.clone().map(|ratio| serde_json::json!(ratio))),
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
    let present: Vec<&str> = ["mask", "size", "seed", "format"]
        .into_iter()
        .filter(|field| image_field_present(request, field))
        .collect();
    identity().reject_unsupported(&present).map_or(Ok(()), Err)
}

/// `decodeResponse` (xai-images.ts:89-110).
pub fn decode(
    output: &TextOutput,
    _context: &DecodeContext<ImageRequest>,
) -> Result<ImageResponse, AiError> {
    let decoded = output.decode_json(output.body)?;
    let data = decoded.get("data").and_then(|value| value.as_array()).cloned().unwrap_or_default();
    let mut images = Vec::new();
    for (index, item) in data.iter().enumerate() {
        let revised_prompt = item.get("revised_prompt").and_then(|value| value.as_str());
        let mut xai = std::collections::BTreeMap::new();
        let metadata = revised_prompt.map(|prompt| {
            xai.insert("revisedPrompt".to_string(), serde_json::json!(prompt));
            let mut metadata = crate::schema::ProviderMetadata::new();
            metadata.insert("xai".to_string(), xai.clone());
            metadata
        });
        images.push(image_output(
            |message| output.invalid(message),
            &format!("xAI Images result {index}"),
            item,
            item.get("mime_type").and_then(|value| value.as_str()),
            None,
            metadata,
        )?);
    }
    if images.is_empty() {
        return Err(output.invalid("xAI Images returned no images"));
    }
    let usage = decoded.get("usage").filter(|value| value.is_object());
    let provider_metadata = usage.map(|usage| {
        let mut xai = std::collections::BTreeMap::new();
        xai.insert("usage".to_string(), usage.clone());
        let mut metadata = crate::schema::ProviderMetadata::new();
        metadata.insert("xai".to_string(), xai);
        metadata
    });
    Ok(ImageResponse { images, usage: None, notices: None, provider_metadata })
}

#[derive(Debug, Clone, Copy, Default)]
pub struct XAIImages;

impl MediaProtocolBase for XAIImages {
    fn identity(&self) -> Identity {
        identity()
    }
    fn unsupported(&self) -> &'static [&'static str] {
        &["mask", "size", "seed", "format"]
    }
    fn base_url(&self) -> String {
        DEFAULT_BASE_URL.to_string()
    }
}

impl InlineProtocol for XAIImages {
    type Request = ImageRequest;
    type Response = ImageResponse;

    fn body_from(&self, request: &ImageRequest) -> Result<Body, AiError> {
        check_unsupported(request)?;
        body_from(request)
    }

    fn path(&self, request: &ImageRequest) -> String {
        if request.images.is_empty() {
            PATH.to_string()
        } else {
            EDIT_PATH.to_string()
        }
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
        ImageRequest { model: "grok-2-image".into(), prompt: "a cat".into(), ..Default::default() }
    }

    #[test]
    fn one_edit_source_lowers_as_image_two_as_images() {
        let mut one = request();
        one.images = vec![media::url("https://x/a.png", None, None)];
        let Body::Json { value, .. } = body_from(&one).unwrap() else { panic!() };
        assert_eq!(value["image"], json!({ "url": "https://x/a.png", "type": "image_url" }));
        assert!(value.get("images").is_none());

        let mut two = request();
        two.images = vec![
            media::url("https://x/a.png", None, None),
            media::reference("xai", "file_9", None),
        ];
        let Body::Json { value, .. } = body_from(&two).unwrap() else { panic!() };
        assert!(value.get("image").is_none());
        assert_eq!(
            value["images"],
            json!([{ "url": "https://x/a.png", "type": "image_url" }, { "file_id": "file_9" }])
        );
    }

    #[test]
    fn aspect_ratio_and_response_format_rename() {
        let mut options = crate::schema::ProviderOptions::new();
        options.insert("responseFormat".to_string(), json!("b64_json"));
        options.insert("resolution".to_string(), json!("2k"));
        let mut image_request = request();
        image_request.aspect_ratio = Some("16:9".into());
        image_request.provider_options = Some(options);
        let Body::Json { value, .. } = body_from(&image_request).unwrap() else { panic!() };
        assert_eq!(value["aspect_ratio"], json!("16:9"));
        assert_eq!(value["response_format"], json!("b64_json"));
        assert_eq!(value["resolution"], json!("2k"));
    }

    #[test]
    fn decodes_outputs_with_revised_prompt_metadata() {
        let identity = identity();
        let body_text = r#"{"data":[{"b64_json":"AQID","revised_prompt":"a better cat","mime_type":"image/jpeg"}],"usage":{"cost_in_usd_ticks":8}}"#;
        let output = TextOutput { identity: &identity, body: body_text, http: None };
        let body = Body::json(Default::default());
        let context = DecodeContext { request: &request(), body: &body };
        let response = decode(&output, &context).unwrap();
        assert_eq!(response.images[0].media_type, "image/jpeg");
        assert_eq!(
            response.images[0].provider_metadata.as_ref().unwrap()["xai"]["revisedPrompt"],
            json!("a better cat")
        );
        assert_eq!(
            response.provider_metadata.as_ref().unwrap()["xai"]["usage"]["cost_in_usd_ticks"],
            json!(8)
        );
        assert!(response.usage.is_none());
    }
}
