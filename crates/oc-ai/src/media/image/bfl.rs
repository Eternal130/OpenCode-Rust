//! `bfl-images` (port of `protocols/bfl-images.ts`). Queued protocol;
//! regional clusters answer on different hosts so the returned
//! `polling_url` is followed verbatim. BFL reports the credit cost on
//! submit; a settled result cost supersedes it.

use std::collections::BTreeMap;

use crate::media::body::{merge_records, Body};
use crate::media::model::{ImageRequest, ImageResponse, PollContext, Snapshot, Started, Status};
use crate::media::protocol::{
    status as table_status, Identity, MediaProtocolBase, QueuedProtocol, TextOutput,
};
use crate::schema::media;
use crate::schema::{AiError, MediaUsage};

pub const ID: &str = "bfl-images";
pub const DEFAULT_BASE_URL: &str = "https://api.bfl.ai";

pub fn identity() -> Identity {
    Identity::new(ID, "Black Forest Labs", "black-forest-labs")
}

/// The serializable token (bfl-images.ts:38-43). `cost` is optional so
/// tokens persisted before it existed still decode.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Token {
    pub id: String,
    pub polling_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
}

/// Model capabilities (bfl-images.ts:81-101).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    pub sizing: Sizing,
    pub image_field: &'static str,
    pub max_images: usize,
    pub mask: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sizing {
    Dimensions,
    AspectRatio,
    None,
}

/// `capabilities(model)` (bfl-images.ts:89-101).
#[must_use]
pub fn capabilities(model: &str) -> Capabilities {
    if model.starts_with("flux-pro-1.0-fill") {
        Capabilities { sizing: Sizing::None, image_field: "image", max_images: 1, mask: true }
    } else if model.starts_with("flux-pro-1.0-expand") {
        Capabilities { sizing: Sizing::None, image_field: "image", max_images: 1, mask: false }
    } else if model.starts_with("flux-kontext") {
        Capabilities {
            sizing: Sizing::AspectRatio,
            image_field: "input_image",
            max_images: 4,
            mask: false,
        }
    } else if model.starts_with("flux-pro-1.1-ultra") {
        Capabilities {
            sizing: Sizing::AspectRatio,
            image_field: "image_prompt",
            max_images: 1,
            mask: false,
        }
    } else if model.starts_with("flux-pro-1.1") || model.starts_with("flux-dev") {
        Capabilities {
            sizing: Sizing::Dimensions,
            image_field: "image_prompt",
            max_images: 1,
            mask: false,
        }
    } else if model.starts_with("flux-2-klein") {
        Capabilities {
            sizing: Sizing::Dimensions,
            image_field: "input_image",
            max_images: 4,
            mask: false,
        }
    } else {
        Capabilities {
            sizing: Sizing::Dimensions,
            image_field: "input_image",
            max_images: 8,
            mask: false,
        }
    }
}

/// `validate` (bfl-images.ts:103-117).
pub fn validate(request: &ImageRequest, model: Capabilities) -> Result<(), AiError> {
    let id = &request.model;
    let images = request.images.len();
    if request.n.is_some_and(|n| n > 1) {
        return Err(identity().unsupported(
            "media.n",
            format!("{id} generates one image per request; call it once per image"),
        ));
    }
    if request.size.is_some() && model.sizing != Sizing::Dimensions {
        return Err(identity()
            .unsupported("media.size", format!("{id} does not take size (width and height)")));
    }
    if request.aspect_ratio.is_some() && model.sizing != Sizing::AspectRatio {
        return Err(
            identity().unsupported("media.aspectRatio", format!("{id} does not take aspectRatio"))
        );
    }
    if images > model.max_images {
        return Err(identity().unsupported(
            "media.images",
            format!("{id} takes at most {} images", model.max_images),
        ));
    }
    if request.mask.is_some() && !model.mask {
        return Err(identity()
            .unsupported("media.mask", format!("{id} does not inpaint; use flux-pro-1.0-fill")));
    }
    Ok(())
}

/// `imageInput` (bfl-images.ts:119-124): inline base64 or https URLs.
fn image_input(asset: &media::Asset) -> Result<String, AiError> {
    asset
        .inline()
        .map(|inline| inline.base64)
        .or_else(|| asset.url().map(str::to_string))
        .ok_or_else(|| {
            crate::protocols::shared::invalid_request(
                "Black Forest Labs accepts inline images or https URLs",
            )
        })
}

/// `fromRequest` (bfl-images.ts:126-149): `input_image` numbers extra
/// references `input_image_2`….
pub fn body_from(request: &ImageRequest) -> Result<Body, AiError> {
    let capabilities = capabilities(&request.model);
    validate(request, capabilities)?;
    let mut images = Vec::new();
    for image in &request.images {
        images.push(image_input(image)?);
    }
    let mut base = crate::media::body::record([
        ("prompt", Some(serde_json::json!(request.prompt))),
        (
            "width",
            request
                .size
                .as_deref()
                .and_then(|size| crate::media::utils::dimensions(size).0)
                .map(crate::media::utils::wire_u64)
                .map(|v| serde_json::json!(v)),
        ),
        (
            "height",
            request
                .size
                .as_deref()
                .and_then(|size| crate::media::utils::dimensions(size).1)
                .map(crate::media::utils::wire_u64)
                .map(|v| serde_json::json!(v)),
        ),
        ("aspect_ratio", request.aspect_ratio.clone().map(|ratio| serde_json::json!(ratio))),
        ("seed", request.seed.map(|seed| serde_json::json!(seed))),
        ("output_format", request.format.clone().map(|format| serde_json::json!(format))),
    ])
    .unwrap_or_default();
    if let Some(mask) = &request.mask {
        base.insert("mask".to_string(), serde_json::json!(image_input(mask)?));
    }
    for (index, image) in images.iter().enumerate() {
        let key = if index == 0 {
            capabilities.image_field.to_string()
        } else {
            format!("{}_{}", capabilities.image_field, index + 1)
        };
        base.insert(key, serde_json::json!(image));
    }
    let merged = merge_records(&[
        Some(&base),
        request.provider_options.as_ref(),
        request.http.as_ref().and_then(|http| http.body.as_ref()),
    ])
    .unwrap_or_default();
    Ok(Body::json(merged))
}

fn status_table() -> BTreeMap<&'static str, Status> {
    [
        ("Pending", Status::Running),
        ("Reasoning", Status::Running),
        ("Generating", Status::Running),
        ("Ready", Status::Completed),
        ("Error", Status::Failed),
        // Moderation is terminal; decode_result reports it as a
        // content-policy failure.
        ("Content Moderated", Status::Failed),
        ("Request Moderated", Status::Failed),
        ("Task not found", Status::Expired),
    ]
    .into_iter()
    .collect()
}

fn is_moderated(status: &str) -> bool {
    status == "Content Moderated" || status == "Request Moderated"
}

/// `decodeStart` (bfl-images.ts:155-162).
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
        token: Token {
            id,
            polling_url: value
                .get("polling_url")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            cost: value.get("cost").and_then(serde_json::Value::as_f64),
        },
    })
}

/// `decodeStatus` (bfl-images.ts:166-172).
pub fn decode_status(
    output: &TextOutput,
    context: &PollContext<Token>,
) -> Result<Snapshot, AiError> {
    let value = output.decode_json(output.body)?;
    let raw = value.get("status").and_then(serde_json::Value::as_str).unwrap_or_default();
    Ok(Snapshot {
        id: context.token.id.clone(),
        status: table_status(&status_table(), raw, output)?,
        progress: None,
        position: None,
    })
}

/// `decodeResult` (bfl-images.ts:174-198). The `sample` URL expires 10
/// minutes after the result is ready, so the route materializes it (the
/// returned asset stays a `url` here; downloading is route-side).
pub fn decode_result(
    output: &TextOutput,
    context: &PollContext<Token>,
) -> Result<ImageResponse, AiError> {
    let value = output.decode_json(output.body)?;
    let raw =
        value.get("status").and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
    let status = table_status(&status_table(), &raw, output)?;
    if is_moderated(&raw) {
        return Err(output.content_policy("Black Forest Labs moderated the generation"));
    }
    if matches!(status, Status::Failed | Status::Expired) {
        return Err(output.ended(
            status,
            format!("Black Forest Labs generation {} ended with {}", context.token.id, raw),
            crate::media::protocol::Failure::ProviderInternal,
        ));
    }
    if status != Status::Completed {
        return Err(output.pending(&context.token.id));
    }
    let Some(result) = value.get("result").filter(|result| !result.is_null()) else {
        return Err(output
            .invalid(format!("Black Forest Labs generation {} has no result", context.token.id)));
    };
    let sample = result.get("sample").and_then(serde_json::Value::as_str).unwrap_or_default();
    // A settled `cost` on the result supersedes the submit-time cost.
    let cost = value.get("cost").and_then(serde_json::Value::as_f64).or(context.token.cost);
    let mut bfl = std::collections::BTreeMap::new();
    bfl.insert("id".to_string(), serde_json::json!(context.token.id));
    bfl.insert(
        "seed".to_string(),
        result.get("seed").filter(|v| !v.is_null()).cloned().unwrap_or(serde_json::Value::Null),
    );
    bfl.insert(
        "prompt".to_string(),
        result.get("prompt").filter(|v| !v.is_null()).cloned().unwrap_or(serde_json::Value::Null),
    );
    if let Some(fields) = result.as_object() {
        for (key, field) in fields {
            if key != "sample" && key != "seed" && key != "prompt" {
                bfl.insert(key.clone(), field.clone());
            }
        }
    }
    let mut metadata = crate::schema::ProviderMetadata::new();
    metadata.insert("bfl".to_string(), bfl);
    Ok(ImageResponse {
        images: vec![media::url(sample, None, None)],
        usage: cost.map(|cost| MediaUsage::Credits { credits: cost }),
        notices: None,
        provider_metadata: Some(metadata),
    })
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BlackForestLabsImages;

impl MediaProtocolBase for BlackForestLabsImages {
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

impl QueuedProtocol for BlackForestLabsImages {
    type Request = ImageRequest;
    type Response = ImageResponse;
    type Token = Token;

    fn body_from(&self, request: &ImageRequest) -> Result<Body, AiError> {
        body_from(request)
    }

    fn start_path(&self, request: &ImageRequest) -> String {
        format!("/v1/{}", request.model)
    }

    fn decode_start(&self, output: &TextOutput) -> Result<Started<Token>, AiError> {
        decode_start(output)
    }

    fn status_path(&self, token: &Token) -> String {
        token.polling_url.clone()
    }

    fn decode_status(
        &self,
        output: &TextOutput,
        context: &PollContext<Token>,
    ) -> Result<Snapshot, AiError> {
        decode_status(output, context)
    }

    fn result_path(&self, token: &Token) -> String {
        token.polling_url.clone()
    }

    fn decode_result(
        &self,
        output: &TextOutput,
        context: &PollContext<Token>,
    ) -> Result<ImageResponse, AiError> {
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
    use serde_json::json;

    fn request(model: &str) -> ImageRequest {
        ImageRequest { model: model.into(), prompt: "a cat".into(), ..Default::default() }
    }

    #[test]
    fn capability_matrix_matches_v2() {
        assert_eq!(
            capabilities("flux-pro-1.0-fill"),
            Capabilities { sizing: Sizing::None, image_field: "image", max_images: 1, mask: true }
        );
        assert_eq!(
            capabilities("flux-kontext-max"),
            Capabilities {
                sizing: Sizing::AspectRatio,
                image_field: "input_image",
                max_images: 4,
                mask: false
            }
        );
        assert_eq!(
            capabilities("flux-pro-1.1-ultra"),
            Capabilities {
                sizing: Sizing::AspectRatio,
                image_field: "image_prompt",
                max_images: 1,
                mask: false
            }
        );
        assert_eq!(
            capabilities("flux-dev"),
            Capabilities {
                sizing: Sizing::Dimensions,
                image_field: "image_prompt",
                max_images: 1,
                mask: false
            }
        );
        assert_eq!(
            capabilities("flux-2-klein"),
            Capabilities {
                sizing: Sizing::Dimensions,
                image_field: "input_image",
                max_images: 4,
                mask: false
            }
        );
        assert_eq!(
            capabilities("other"),
            Capabilities {
                sizing: Sizing::Dimensions,
                image_field: "input_image",
                max_images: 8,
                mask: false
            }
        );
    }

    #[test]
    fn sizing_dimensions_lower_width_height() {
        let mut image_request = request("flux-dev");
        image_request.size = Some("1024x768".into());
        image_request.seed = Some(5.0);
        let Body::Json { value, .. } = body_from(&image_request).unwrap() else { panic!() };
        assert_eq!(value["width"], json!(1024));
        assert_eq!(value["height"], json!(768));
        assert_eq!(value["seed"], json!(5));
        assert!(value.get("aspect_ratio").is_none());
    }

    #[test]
    fn kontext_numbers_extra_images() {
        let mut image_request = request("flux-kontext-max");
        image_request.aspect_ratio = Some("16:9".into());
        image_request.images =
            vec![media::url("https://x/1.png", None, None), media::base64("AQID", "image/png")];
        let Body::Json { value, .. } = body_from(&image_request).unwrap() else { panic!() };
        assert_eq!(value["aspect_ratio"], json!("16:9"));
        assert_eq!(value["input_image"], json!("https://x/1.png"));
        assert_eq!(value["input_image_2"], json!("AQID"));
    }

    #[test]
    fn validation_rejections_are_typed() {
        let mut image_request = request("flux-dev");
        image_request.aspect_ratio = Some("16:9".into());
        let error = body_from(&image_request).unwrap_err();
        assert!(matches!(error.reason, crate::schema::AiErrorReason::UnsupportedOperation { .. }));

        let mut mask_request = request("flux-dev");
        mask_request.mask = Some(media::base64("AQID", "image/png"));
        let error = body_from(&mask_request).unwrap_err();
        assert!(error.message().contains("does not inpaint; use flux-pro-1.0-fill"));
    }

    #[test]
    fn decode_result_maps_costs_and_moderation() {
        let identity = identity();
        let auth = BTreeMap::new();
        let token = Token { id: "t1".into(), polling_url: "https://poll".into(), cost: Some(1.0) };
        let context = PollContext { token: &token, auth: &auth };

        let ready = TextOutput {
            identity: &identity,
            body: r#"{"id":"t1","status":"Ready","cost":2.5,"result":{"sample":"https://s/1.png","seed":9,"prompt":"a cat","extra":true}}"#,
            http: None,
        };
        let response = decode_result(&ready, &context).unwrap();
        assert_eq!(response.images[0].url(), Some("https://s/1.png"));
        assert_eq!(response.usage, Some(MediaUsage::Credits { credits: 2.5 }));
        assert_eq!(response.provider_metadata.as_ref().unwrap()["bfl"]["seed"], json!(9));
        assert_eq!(response.provider_metadata.as_ref().unwrap()["bfl"]["extra"], json!(true));

        let moderated = TextOutput {
            identity: &identity,
            body: r#"{"id":"t1","status":"Content Moderated"}"#,
            http: None,
        };
        let error = decode_result(&moderated, &context).unwrap_err();
        assert!(matches!(error.reason, crate::schema::AiErrorReason::ContentPolicy { .. }));

        let pending = TextOutput {
            identity: &identity,
            body: r#"{"id":"t1","status":"Generating"}"#,
            http: None,
        };
        let error = decode_result(&pending, &context).unwrap_err();
        assert!(error.message().contains("has not finished"));
    }
}
