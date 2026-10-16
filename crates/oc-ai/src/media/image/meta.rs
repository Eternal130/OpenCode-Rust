//! `meta-images` (port of `protocols/meta-images.ts`). Inline protocol;
//! Meta has no file handles so refs are rejected even when they name this
//! provider.

use crate::media::body::{merge_records, Body};
use crate::media::model::{image_field_present, DecodeContext, ImageRequest, ImageResponse};
use crate::media::protocol::{Identity, InlineProtocol, MediaProtocolBase, TextOutput};
use crate::media::utils::{self, media_reference};
use crate::schema::media::{self, Info};
use crate::schema::{AiError, MediaUsage};

pub const ID: &str = "meta-images";
pub const DEFAULT_BASE_URL: &str = "https://api.meta.ai/v1";

pub fn identity() -> Identity {
    Identity::new(ID, "Meta Images", "meta")
}

fn reference(asset: &media::Asset) -> Result<serde_json::Value, AiError> {
    let (_, value) = media_reference(asset, None, "Meta Images")?;
    Ok(serde_json::json!({ "image_url": value }))
}

/// `fromRequest` (meta-images.ts:74-95).
pub fn body_from(request: &ImageRequest) -> Result<Body, AiError> {
    let mut references = Vec::new();
    for image in &request.images {
        references.push(reference(image)?);
    }
    let mut native = request.provider_options.clone().unwrap_or_default();
    let response_format = native.remove("responseFormat");
    let reasoning_strength = native.remove("reasoningStrength");
    let tool_enablement = native.remove("toolEnablement");

    let base = crate::media::body::record([
        ("model", Some(serde_json::json!(request.model))),
        ("prompt", Some(serde_json::json!(request.prompt))),
        ("images", (!references.is_empty()).then(|| serde_json::json!(references))),
        ("n", request.n.map(|n| serde_json::json!(n))),
        ("size", request.size.clone().map(|size| serde_json::json!(size))),
        ("output_format", request.format.clone().map(|format| serde_json::json!(format))),
        ("response_format", response_format),
        ("reasoning_strength", reasoning_strength),
        ("tool_enablement", tool_enablement),
    ]);
    let merged = merge_records(&[base.as_ref(), Some(&native), request.http.as_ref().and_then(|h| h.body.as_ref())])
        .unwrap_or_default();
    Ok(Body::json(merged))
}

/// The route's unsupported-field rejection.
pub fn check_unsupported(request: &ImageRequest) -> Result<(), AiError> {
    let present: Vec<&str> = ["mask", "aspectRatio", "seed"]
        .into_iter()
        .filter(|field| image_field_present(request, field))
        .collect();
    identity().reject_unsupported(&present).map_or(Ok(()), Err)
}

/// `decodeResponse` (meta-images.ts:103-132).
pub fn decode(output: &TextOutput, context: &DecodeContext<ImageRequest>) -> Result<ImageResponse, AiError> {
    let decoded = output.decode_json(output.body)?;
    let requested = context
        .body
        .json_field("output_format")
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let format = decoded
        .get("output_format")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .or(requested)
        .unwrap_or_else(|| "webp".to_string());
    let media_type = format!("image/{format}");
    let data = decoded.get("data").and_then(|value| value.as_array()).cloned().unwrap_or_default();
    let mut images = Vec::new();
    for (index, item) in data.iter().enumerate() {
        images.push(utils::image_output(
            |message| output.invalid(message),
            &format!("Meta Images result {index}"),
            item,
            Some(&media_type),
            Some(Info { format: Some(format.clone()), ..Info::default() }),
            None,
        )?);
    }
    if images.is_empty() {
        return Err(output.invalid("Meta Images returned no images"));
    }
    let usage = decoded.get("usage").filter(|value| !value.is_null()).map(|value| MediaUsage::Tokens {
        input: value.get("input_tokens").and_then(|v| v.as_f64()),
        output: value.get("output_tokens").and_then(|v| v.as_f64()),
        total: value.get("total_tokens").and_then(|v| v.as_f64()),
        details: Some(serde_json::from_value(value.clone()).unwrap_or_default()),
    });
    let mut meta = std::collections::BTreeMap::new();
    meta.insert("outputFormat".to_string(), serde_json::json!(format));
    let mut metadata = crate::schema::ProviderMetadata::new();
    metadata.insert("meta".to_string(), meta);
    Ok(ImageResponse { images, usage, notices: None, provider_metadata: Some(metadata) })
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MetaImages;

impl MediaProtocolBase for MetaImages {
    fn identity(&self) -> Identity {
        identity()
    }
    fn unsupported(&self) -> &'static [&'static str] {
        &["mask", "aspectRatio", "seed"]
    }
    fn base_url(&self) -> String {
        DEFAULT_BASE_URL.to_string()
    }
}

impl InlineProtocol for MetaImages {
    type Request = ImageRequest;
    type Response = ImageResponse;

    fn body_from(&self, request: &ImageRequest) -> Result<Body, AiError> {
        check_unsupported(request)?;
        body_from(request)
    }

    fn path(&self, request: &ImageRequest) -> String {
        if request.images.is_empty() {
            "/images/generations".to_string()
        } else {
            "/images/edits".to_string()
        }
    }

    fn decode(&self, output: &TextOutput, context: &DecodeContext<ImageRequest>) -> Result<ImageResponse, AiError> {
        decode(output, context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request() -> ImageRequest {
        ImageRequest { model: "meta-image-1".into(), prompt: "a cat".into(), ..Default::default() }
    }

    #[test]
    fn lowers_the_generation_body() {
        let body = body_from(&request()).unwrap();
        let Body::Json { value, .. } = body else { panic!() };
        assert_eq!(value["model"], json!("meta-image-1"));
        assert_eq!(value["prompt"], json!("a cat"));
        assert!(value.get("images").is_none());
    }

    #[test]
    fn edit_sources_become_image_urls() {
        let mut image_request = request();
        image_request.images = vec![
            media::url("https://x/a.png", None, None),
            media::base64("AQID", "image/png"),
        ];
        let body = body_from(&image_request).unwrap();
        let Body::Json { value, .. } = body else { panic!() };
        assert_eq!(
            value["images"],
            json!([{ "image_url": "https://x/a.png" }, { "image_url": "data:image/png;base64,AQID" }])
        );
    }

    #[test]
    fn camel_case_native_options_rename() {
        let mut options = crate::schema::ProviderOptions::new();
        options.insert("responseFormat".to_string(), json!("url"));
        options.insert("reasoningStrength".to_string(), json!("high"));
        let mut image_request = request();
        image_request.provider_options = Some(options);
        let body = body_from(&image_request).unwrap();
        let Body::Json { value, .. } = body else { panic!() };
        assert_eq!(value["response_format"], json!("url"));
        assert_eq!(value["reasoning_strength"], json!("high"));
    }

    #[test]
    fn decodes_b64_and_url_outputs_with_usage() {
        let identity = identity();
        let body_text = r#"{"data":[{"b64_json":"AQID"},{"url":"https://x/1.webp"}],"output_format":"webp","usage":{"input_tokens":3,"output_tokens":4,"total_tokens":7}}"#;
        let output = TextOutput { identity: &identity, body: body_text, http: None };
        let body = Body::json(Default::default());
        let context = DecodeContext { request: &request(), body: &body };
        let response = decode(&output, &context).unwrap();
        assert_eq!(response.images.len(), 2);
        assert_eq!(response.images[0].media_type, "image/webp");
        assert_eq!(response.images[1].url(), Some("https://x/1.webp"));
        assert_eq!(response.provider_metadata.as_ref().unwrap()["meta"]["outputFormat"], json!("webp"));
        assert!(matches!(response.usage, Some(MediaUsage::Tokens { total: Some(7.0), .. })));
    }

    #[test]
    fn requested_format_falls_back_through_the_body() {
        let identity = identity();
        let body_text = r#"{"data":[{"url":"https://x/1"}]}"#;
        let output = TextOutput { identity: &identity, body: body_text, http: None };
        let mut json_body = crate::schema::options::JsonRecord::new();
        json_body.insert("output_format".to_string(), json!("jpeg"));
        let body = Body::json(json_body);
        let context = DecodeContext { request: &request(), body: &body };
        let response = decode(&output, &context).unwrap();
        assert_eq!(response.images[0].media_type, "image/jpeg");
    }
}
