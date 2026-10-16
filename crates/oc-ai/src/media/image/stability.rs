//! `stability-images` (port of `protocols/stability-images.ts`). Inline
//! generate protocol + a queued creative-upscale protocol. Responses are
//! base64 JSON instead of raw bytes so the seed and finish reason arrive
//! with the image.

use std::collections::BTreeMap;

use crate::media::body::{merge_records, Body, Form};
use crate::media::model::{
    image_field_present, ImageRequest, ImageResponse, PollContext, Snapshot, Started, Status,
};
use crate::media::protocol::{
    Identity, InlineProtocol, MediaProtocolBase, QueuedProtocol, TextOutput,
};
use crate::media::utils::{self, RepeatArrays};
use crate::schema::media::{self, Notice, NoticeType};
use crate::schema::{AiError, HttpContext};

pub const ID: &str = "stability-images";
pub const UPSCALE_ID: &str = "stability-upscale";
pub const DEFAULT_BASE_URL: &str = "https://api.stability.ai";
pub const RESULTS_PATH: &str = "/v2beta/results";
/// Only the creative upscaler is asynchronous.
pub const UPSCALE_MODEL: &str = "creative";
/// Base64 JSON instead of raw bytes (stability-images.ts:16).
pub const HEADERS: &[(&str, &str)] = &[("accept", "application/json")];

pub fn identity() -> Identity {
    Identity::new(ID, "Stability AI", "stability")
}

pub fn upscale_identity() -> Identity {
    Identity::new(UPSCALE_ID, "Stability AI", "stability")
}

/// `endpoint(model)` (stability-images.ts:81): `sd3.5-*` ids share the
/// `sd3` endpoint and select the model with a form field.
#[must_use]
pub fn endpoint(model: &str) -> &str {
    if model.starts_with("sd3") {
        "sd3"
    } else {
        model
    }
}

/// `validate` + `form` (stability-images.ts:85-121).
pub fn body_from(request: &ImageRequest) -> Result<Body, AiError> {
    if request.n.is_some_and(|n| n > 1) {
        return Err(identity().unsupported(
            "media.n",
            "Stability AI generates one image per request; call it once per image",
        ));
    }
    let target = endpoint(&request.model);
    let edit = !request.images.is_empty();
    if edit && target == "core" {
        return Err(identity().unsupported(
            "media.images",
            "Stability AI core is text-to-image only; use ultra or sd3.5-*",
        ));
    }
    form_body(
        &identity(),
        crate::media::body::record([
            ("prompt", Some(serde_json::json!(request.prompt))),
            ("aspect_ratio", request.aspect_ratio.clone().map(|ratio| serde_json::json!(ratio))),
            ("seed", request.seed.map(|seed| serde_json::json!(seed))),
            ("output_format", request.format.clone().map(|format| serde_json::json!(format))),
            ("model", (target == "sd3").then(|| serde_json::json!(request.model))),
            ("mode", (target == "sd3" && edit).then(|| serde_json::json!("image-to-image"))),
        ])
        .as_ref(),
        &merge_records(&[
            request.provider_options.as_ref(),
            request.http.as_ref().and_then(|http| http.body.as_ref()),
        ]),
        &request.images,
    )
}

/// `form` (stability-images.ts:85-99).
fn form_body(
    identity: &Identity,
    fields: Option<&crate::schema::options::JsonRecord>,
    native: &Option<crate::schema::options::JsonRecord>,
    source: &[media::Asset],
) -> Result<Body, AiError> {
    if source.len() > 1 {
        return Err(identity
            .unsupported("media.images", format!("{} takes one source image", identity.name)));
    }
    let mut form = Form::new();
    let owned: Vec<(String, serde_json::Value)> = fields
        .map(|record| record.iter().map(|(key, value)| (key.clone(), value.clone())).collect())
        .unwrap_or_default();
    utils::append_fields(
        &mut form,
        owned,
        native.as_ref(),
        &["image", "prompt", "mode", "model"],
        None,
    );
    if let Some(image) = source.first() {
        let data = utils::inline_bytes(identity.id, image)?;
        form.append_file("image", "image", image.media_type.clone(), data);
    }
    Ok(Body::multipart(form))
}

/// `decodeImageDocument` (stability-images.ts:138-155): generate names the
/// base64 image `image`; `/v2beta/results/{id}` names it `result`.
pub fn decode_image_document(
    identity: &Identity,
    output: &TextOutput,
) -> Result<ImageResponse, AiError> {
    let document = output.decode_json(output.body)?;
    let data = document
        .get("image")
        .and_then(|value| value.as_str())
        .or_else(|| document.get("result").and_then(|value| value.as_str()));
    let Some(data) = data else {
        return Err(output.invalid(format!("{} returned no image", identity.name)));
    };
    let image = utils::decoded_asset(
        |message| output.invalid(message),
        &format!("{} result", identity.name),
        data,
        None,
        None,
        None,
    )?;
    let mut stability = std::collections::BTreeMap::new();
    stability.insert("seed".to_string(), document.get("seed").cloned().unwrap_or_default());
    stability.insert(
        "finishReason".to_string(),
        document.get("finish_reason").cloned().unwrap_or_default(),
    );
    let mut metadata = crate::schema::ProviderMetadata::new();
    metadata.insert("stability".to_string(), stability);
    Ok(ImageResponse {
        images: vec![image],
        notices: (document.get("finish_reason").and_then(|v| v.as_str())
            == Some("CONTENT_FILTERED"))
        .then(|| {
            vec![Notice {
                kind: NoticeType::Moderated,
                message: format!(
                    "{} blurred the image for violating its content policy",
                    identity.name
                ),
                provider_metadata: None,
            }]
        }),
        usage: None,
        provider_metadata: Some(metadata),
    })
}

/// The generate protocol.
#[derive(Debug, Clone, Copy, Default)]
pub struct StabilityImages;

impl MediaProtocolBase for StabilityImages {
    fn identity(&self) -> Identity {
        identity()
    }
    fn unsupported(&self) -> &'static [&'static str] {
        &["size", "mask"]
    }
    fn base_url(&self) -> String {
        DEFAULT_BASE_URL.to_string()
    }
    fn headers(&self) -> Vec<(String, String)> {
        HEADERS.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }
}

impl InlineProtocol for StabilityImages {
    type Request = ImageRequest;
    type Response = ImageResponse;

    fn body_from(&self, request: &ImageRequest) -> Result<Body, AiError> {
        body_from(request)
    }

    fn path(&self, request: &ImageRequest) -> String {
        format!("/v2beta/stable-image/generate/{}", endpoint(&request.model))
    }

    fn decode(
        &self,
        output: &TextOutput,
        _context: &crate::media::model::DecodeContext<ImageRequest>,
    ) -> Result<ImageResponse, AiError> {
        decode_image_document(&identity(), output)
    }
}

/// The creative-upscale token (stability-images.ts:63).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpscaleToken {
    pub id: String,
}

/// The upscale protocol (stability-images.ts:193-199).
#[derive(Debug, Clone, Copy, Default)]
pub struct StabilityUpscale;

/// `fromUpscaleRequest` (stability-images.ts:123-132).
pub fn upscale_body_from(request: &ImageRequest) -> Result<Body, AiError> {
    if request.images.is_empty() {
        return Err(crate::protocols::shared::invalid_request(
            "Stability AI upscale requires the source image in images",
        ));
    }
    form_body(
        &upscale_identity(),
        crate::media::body::record([
            ("prompt", Some(serde_json::json!(request.prompt))),
            ("seed", request.seed.map(|seed| serde_json::json!(seed))),
            ("output_format", request.format.clone().map(|format| serde_json::json!(format))),
        ])
        .as_ref(),
        &merge_records(&[
            request.provider_options.as_ref(),
            request.http.as_ref().and_then(|http| http.body.as_ref()),
        ]),
        &request.images,
    )
}

/// `decodeStart` (stability-images.ts:160-163).
pub fn upscale_decode_start(output: &TextOutput) -> Result<Started<UpscaleToken>, AiError> {
    let value = output.decode_json(output.body)?;
    let id = value.get("id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    Ok(Started {
        snapshot: Snapshot {
            id: id.clone(),
            status: Status::Queued,
            progress: None,
            position: None,
        },
        token: UpscaleToken { id },
    })
}

/// `decodeStatus` (stability-images.ts:166-170): `/v2beta/results/{id}`
/// answers 202 while in progress.
pub fn upscale_decode_status(
    http: Option<&HttpContext>,
    context: &PollContext<UpscaleToken>,
) -> Result<Snapshot, AiError> {
    let status = if http.is_some_and(|http| http.status == 202) {
        Status::Running
    } else {
        Status::Completed
    };
    Ok(Snapshot { id: context.token.id.clone(), status, progress: None, position: None })
}

/// `decodeUpscaleResult` (stability-images.ts:172-181).
pub fn upscale_decode_result(
    output: &TextOutput,
    http: Option<&HttpContext>,
    context: &PollContext<UpscaleToken>,
) -> Result<ImageResponse, AiError> {
    if http.is_some_and(|http| http.status == 202) {
        return Err(output.pending(&context.token.id));
    }
    decode_image_document(&upscale_identity(), output)
}

impl MediaProtocolBase for StabilityUpscale {
    fn identity(&self) -> Identity {
        upscale_identity()
    }
    fn unsupported(&self) -> &'static [&'static str] {
        &["n", "size", "aspectRatio", "mask"]
    }
    fn base_url(&self) -> String {
        DEFAULT_BASE_URL.to_string()
    }
    fn headers(&self) -> Vec<(String, String)> {
        HEADERS.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }
}

impl QueuedProtocol for StabilityUpscale {
    type Request = ImageRequest;
    type Response = ImageResponse;
    type Token = UpscaleToken;

    fn body_from(&self, request: &ImageRequest) -> Result<Body, AiError> {
        upscale_body_from(request)
    }

    fn start_path(&self, _request: &ImageRequest) -> String {
        format!("/v2beta/stable-image/upscale/{UPSCALE_MODEL}")
    }

    fn decode_start(&self, output: &TextOutput) -> Result<Started<UpscaleToken>, AiError> {
        upscale_decode_start(output)
    }

    fn status_path(&self, token: &UpscaleToken) -> String {
        format!("{RESULTS_PATH}/{}", token.id)
    }

    fn decode_status(
        &self,
        output: &TextOutput,
        context: &PollContext<UpscaleToken>,
    ) -> Result<Snapshot, AiError> {
        upscale_decode_status(output.http, context)
    }

    fn result_path(&self, token: &UpscaleToken) -> String {
        format!("{RESULTS_PATH}/{}", token.id)
    }

    fn decode_result(
        &self,
        output: &TextOutput,
        context: &PollContext<UpscaleToken>,
    ) -> Result<ImageResponse, AiError> {
        upscale_decode_result(output, output.http, context)
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
    fn generate_lowers_the_multipart_form() {
        let mut image_request = request("core");
        image_request.aspect_ratio = Some("3:2".into());
        image_request.seed = Some(11.0);
        let Body::Multipart(form) = body_from(&image_request).unwrap() else { panic!() };
        assert_eq!(form.fields_json()["prompt"], json!("a cat"));
        assert_eq!(form.fields_json()["aspect_ratio"], json!("3:2"));
        assert_eq!(form.fields_json()["seed"], json!(11));
        assert!(form.files().is_empty());
    }

    #[test]
    fn sd35_shares_the_sd3_endpoint_with_model_field() {
        let mut image_request = request("sd3.5-large");
        image_request.images = vec![media::base64("AQID", "image/png")];
        let Body::Multipart(form) = body_from(&image_request).unwrap() else { panic!() };
        assert_eq!(form.fields_json()["model"], json!("sd3.5-large"));
        assert_eq!(form.fields_json()["mode"], json!("image-to-image"));
        assert_eq!(form.files(), vec![("image", "image", "image/png")]);
        assert_eq!(StabilityImages.path(&image_request), "/v2beta/stable-image/generate/sd3");
    }

    #[test]
    fn core_rejects_edit_sources() {
        let mut image_request = request("core");
        image_request.images = vec![media::base64("AQID", "image/png")];
        let error = body_from(&image_request).unwrap_err();
        assert!(error.message().contains("text-to-image only"));
    }

    #[test]
    fn reserved_form_fields_survive_overlays() {
        let mut options = crate::schema::ProviderOptions::new();
        options.insert("model".to_string(), json!("hijack"));
        options.insert("style_preset".to_string(), json!("anime"));
        let mut image_request = request("ultra");
        image_request.provider_options = Some(options);
        let Body::Multipart(form) = body_from(&image_request).unwrap() else { panic!() };
        assert!(form.fields_json().get("model").is_none());
        assert_eq!(form.fields_json()["style_preset"], json!("anime"));
    }

    #[test]
    fn image_document_decodes_with_content_filter_notice() {
        let identity = identity();
        let body = r#"{"image":"AQID","seed":42,"finish_reason":"CONTENT_FILTERED"}"#;
        let output = TextOutput { identity: &identity, body, http: None };
        let response = decode_image_document(&identity, &output).unwrap();
        assert_eq!(response.images.len(), 1);
        assert_eq!(response.provider_metadata.as_ref().unwrap()["stability"]["seed"], json!(42));
        assert_eq!(response.notices.as_ref().unwrap()[0].kind, NoticeType::Moderated);
    }

    #[test]
    fn upscale_status_reads_202_as_running() {
        let auth = BTreeMap::new();
        let token = UpscaleToken { id: "u1".into() };
        let context = PollContext { token: &token, auth: &auth };
        let http = HttpContext { url: "https://x".into(), status: 202, headers: BTreeMap::new() };
        assert_eq!(upscale_decode_status(Some(&http), &context).unwrap().status, Status::Running);
        let http200 =
            HttpContext { url: "https://x".into(), status: 200, headers: BTreeMap::new() };
        assert_eq!(
            upscale_decode_status(Some(&http200), &context).unwrap().status,
            Status::Completed
        );
    }
}
