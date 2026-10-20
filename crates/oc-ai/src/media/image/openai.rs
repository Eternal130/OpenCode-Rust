//! `openai-images` (port of `protocols/openai-images.ts`).
//!
//! Stream protocol: `generate` reads the whole JSON response as one
//! document frame; `stream` mode adds `stream: true` + `partial_images`
//! and parses SSE `image_generation.*` / `image_edit.*` events. Owned
//! bytes go through multipart edits; URLs and file ids use the JSON body.

use crate::media::body::{merge_records, Body, Form};
use crate::media::model::{
    image_field_present, Addressed, ImageEvent, ImageRequest, Mode, ResponseContext,
};
use crate::media::protocol::{
    Frame, Framing, Identity, MediaProtocolBase, StreamProtocol, TextOutput,
};
use crate::media::utils::{self, media_reference, MediaRefKind};
use crate::schema::media::{self, Info};
use crate::schema::{AiError, MediaUsage, ProviderId};

pub const ID: &str = "openai-images";
pub const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
pub const PATH: &str = "/images/generations";
pub const EDIT_PATH: &str = "/images/edits";

pub fn identity() -> Identity {
    Identity::new(ID, "OpenAI Images", "openai")
}

/// Multipart field names the route owns; `http.body` overlays cannot
/// smuggle replacements for them (openai-images.ts:110).
const RESERVED_FORM_FIELDS: &[&str] = &["model", "prompt", "image", "image[]", "images", "mask"];

fn native_options(options: &crate::schema::ProviderOptions) -> crate::schema::ProviderOptions {
    let mut native = options.clone();
    let output_compression = native.remove("outputCompression");
    let partial_images = native.remove("partialImages");
    if let Some(compression) = output_compression {
        native.insert("output_compression".to_string(), compression);
    }
    let _ = partial_images;
    native
}

/// `streamOptions` (openai-images.ts:118-125): stream mode asks for SSE
/// partials; more than one image is a typed rejection.
fn stream_options(
    request: &Addressed<ImageRequest>,
) -> Result<Option<crate::schema::ProviderOptions>, AiError> {
    if request.mode != Mode::Stream {
        return Ok(None);
    }
    if let Some(n) = request.request.n {
        if n > 1 {
            return Err(identity().unsupported(
                "media.n",
                format!("OpenAI Images streams one image; use Image.generate for n={n}"),
            ));
        }
    }
    let mut options = crate::schema::ProviderOptions::new();
    options.insert("stream".to_string(), serde_json::json!(true));
    let partial = request
        .request
        .provider_options
        .as_ref()
        .and_then(|options| options.get("partialImages"))
        .cloned()
        .unwrap_or(serde_json::json!(2));
    options.insert("partial_images".to_string(), partial);
    Ok(Some(options))
}

fn is_inline(asset: &media::Asset) -> bool {
    matches!(asset.source, media::Source::Bytes { .. } | media::Source::Base64 { .. })
}

fn reference(asset: &media::Asset) -> Result<serde_json::Value, AiError> {
    let (kind, value) = media_reference(asset, Some(&ProviderId::new("openai")), "OpenAI Images")?;
    Ok(match kind {
        MediaRefKind::Ref => serde_json::json!({ "file_id": value }),
        _ => serde_json::json!({ "image_url": value }),
    })
}

/// `fromRequest` (openai-images.ts:136-177).
pub fn body_from(request: &Addressed<ImageRequest>) -> Result<Body, AiError> {
    let images = &request.request.images;
    let mask = &request.request.mask;
    if mask.is_some() && images.is_empty() {
        return Err(crate::protocols::shared::invalid_request(
            "An OpenAI image mask requires at least one input image",
        ));
    }
    let fields = merge_records(&[
        crate::media::body::record([
            ("n", request.request.n.map(|n| serde_json::json!(n))),
            ("size", request.request.size.clone().map(|size| serde_json::json!(size))),
            (
                "output_format",
                request.request.format.clone().map(|format| serde_json::json!(format)),
            ),
        ])
        .as_ref(),
        stream_options(request)?.as_ref(),
        Some(&native_options(
            request.request.provider_options.as_ref().unwrap_or(&Default::default()),
        )),
        request.request.http.as_ref().and_then(|http| http.body.as_ref()),
    ]);

    // Owned bytes go through multipart edits; remote URLs and file IDs use
    // the JSON edits body instead (openai-images.ts:148-162).
    let all_inline =
        !images.is_empty() && images.iter().all(is_inline) && mask.as_ref().is_none_or(is_inline);
    if all_inline {
        let mut form = Form::new();
        let owned: Vec<(String, serde_json::Value)> = vec![
            ("model".to_string(), serde_json::json!(request.request.model)),
            ("prompt".to_string(), serde_json::json!(request.request.prompt)),
        ];
        utils::append_fields(&mut form, owned, fields.as_ref(), RESERVED_FORM_FIELDS, None);
        for (index, image) in images.iter().enumerate() {
            let data = utils::inline_bytes(ID, image)?;
            form.append_file("image[]", format!("image-{index}"), image.media_type.clone(), data);
        }
        if let Some(mask) = mask {
            let data = utils::inline_bytes(ID, mask)?;
            form.append_file("mask", "mask", mask.media_type.clone(), data);
        }
        return Ok(Body::multipart(form));
    }

    let mut references = Vec::new();
    for image in images {
        references.push(reference(image)?);
    }
    let mask_reference = match mask {
        Some(mask) => Some(reference(mask)?),
        None => None,
    };
    let base = crate::media::body::record([
        ("model", Some(serde_json::json!(request.request.model))),
        ("prompt", Some(serde_json::json!(request.request.prompt))),
        ("images", (!references.is_empty()).then(|| serde_json::json!(references))),
        ("mask", mask_reference),
    ]);
    let merged = merge_records(&[base.as_ref(), fields.as_ref()]).unwrap_or_default();
    Ok(Body::json(merged))
}

/// The route's unsupported-field rejection before `body_from`.
pub fn check_unsupported(request: &ImageRequest) -> Result<(), AiError> {
    let present: Vec<&str> = ["aspectRatio", "seed"]
        .into_iter()
        .filter(|field| image_field_present(request, field))
        .collect();
    identity().reject_unsupported(&present).map_or(Ok(()), Err)
}

/// Parser state (openai-images.ts:96-103).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub completed: usize,
    pub format: Option<String>,
    pub size: Option<String>,
    pub quality: Option<String>,
    pub background: Option<String>,
    pub usage: Option<MediaUsage>,
}

/// `usage` mapping (openai-images.ts:189-198).
fn usage_of(value: &serde_json::Value) -> Option<MediaUsage> {
    if value.is_null() {
        return None;
    }
    Some(MediaUsage::Tokens {
        input: value.get("input_tokens").and_then(serde_json::Value::as_f64),
        output: value.get("output_tokens").and_then(serde_json::Value::as_f64),
        total: value.get("total_tokens").and_then(serde_json::Value::as_f64),
        details: Some(
            serde_json::from_value::<serde_json::Map<String, serde_json::Value>>(value.clone())
                .unwrap_or_default(),
        ),
    })
}

/// `info` (openai-images.ts:201-204): `size` echoes the rendered
/// `WIDTHxHEIGHT`; `auto` or any other value leaves dimensions unknown.
fn info_of(format: &str, size: Option<&str>) -> Info {
    let (width, height) = size.map_or((None, None), utils::dimensions);
    Info { format: Some(format.to_string()), width, height, ..Info::default() }
}

fn event_image(
    invalid: impl FnOnce(String) -> AiError,
    label: &str,
    data: &str,
    format: &str,
    size: Option<&str>,
) -> Result<media::Asset, AiError> {
    utils::decoded_asset(
        invalid,
        label,
        data,
        Some(&format!("image/{format}")),
        Some(info_of(format, size)),
        None,
    )
}

/// `onEvent` (openai-images.ts:211-230) — one SSE frame.
pub fn on_event(state: &mut State, frame: &str) -> Result<Vec<ImageEvent>, AiError> {
    let invalid = |message: String| identity().frame_error(message, frame.to_string());
    let event =
        TextOutput { identity: &identity(), body: frame, http: None }.decode_frame(frame)?;
    let event_type = event.get("type").and_then(serde_json::Value::as_str).unwrap_or_default();
    let format = event
        .get("output_format")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    if event_type.ends_with("partial_image") {
        let data = event.get("b64_json").and_then(serde_json::Value::as_str).unwrap_or_default();
        let index = event
            .get("partial_image_index")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or_default();
        let image = event_image(
            invalid,
            "OpenAI Images partial image",
            data,
            &format,
            event.get("size").and_then(serde_json::Value::as_str),
        )?;
        return Ok(vec![ImageEvent::ImagePartial { index, image }]);
    }
    let data = event.get("b64_json").and_then(serde_json::Value::as_str).unwrap_or_default();
    let image = event_image(
        invalid,
        &format!("OpenAI Images result {}", state.completed),
        data,
        &format,
        event.get("size").and_then(serde_json::Value::as_str),
    )?;
    let index = state.completed as f64;
    state.completed += 1;
    state.format = Some(format);
    state.size = event.get("size").and_then(serde_json::Value::as_str).map(str::to_string);
    state.quality = event.get("quality").and_then(serde_json::Value::as_str).map(str::to_string);
    state.background =
        event.get("background").and_then(serde_json::Value::as_str).map(str::to_string);
    state.usage = usage_of(event.get("usage").unwrap_or(&serde_json::Value::Null));
    Ok(vec![ImageEvent::Image { index, image }])
}

/// `onDocument` (openai-images.ts:232-253) — the `generate` frame.
pub fn on_document(
    state: &mut State,
    document: &str,
    requested: Option<&str>,
) -> Result<Vec<ImageEvent>, AiError> {
    let invalid = |message: String| {
        AiError::new(crate::schema::AiErrorReason::InvalidProviderOutput {
            fields: crate::schema::ReasonFields {
                message,
                body: Some(document.to_string()),
                http: None,
                cause: None,
            },
            classification: None,
            route: Some(ID.to_string()),
        })
    };
    let decoded = serde_json::from_str::<serde_json::Value>(document)
        .map_err(|cause| invalid(format!("OpenAI Images returned an invalid response: {cause}")))?;
    let format = decoded
        .get("output_format")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .or_else(|| requested.map(str::to_string))
        .unwrap_or_else(|| "png".to_string());
    let size = decoded.get("size").and_then(serde_json::Value::as_str);
    let data =
        decoded.get("data").and_then(serde_json::Value::as_array).cloned().unwrap_or_default();
    let mut images = Vec::new();
    for (index, item) in data.iter().enumerate() {
        let b64 = item.get("b64_json").and_then(serde_json::Value::as_str).unwrap_or_default();
        images.push(utils::decoded_asset(
            invalid,
            &format!("OpenAI Images result {index}"),
            b64,
            Some(&format!("image/{format}")),
            Some(info_of(&format, size)),
            None,
        )?);
    }
    if images.is_empty() {
        return Err(invalid("OpenAI Images returned no images".to_string()));
    }
    state.completed = images.len();
    state.format = Some(format);
    state.size = size.map(str::to_string);
    state.quality = decoded.get("quality").and_then(serde_json::Value::as_str).map(str::to_string);
    state.background =
        decoded.get("background").and_then(serde_json::Value::as_str).map(str::to_string);
    state.usage = usage_of(decoded.get("usage").unwrap_or(&serde_json::Value::Null));
    Ok(images
        .into_iter()
        .enumerate()
        .map(|(index, image)| ImageEvent::Image { index: index as f64, image })
        .collect())
}

/// `step` (openai-images.ts:255).
pub fn step(state: &mut State, frame: &Frame) -> Result<Vec<ImageEvent>, AiError> {
    match frame {
        Frame::Text(text) => on_event(state, text),
        Frame::Document { document, requested } => {
            on_document(state, document, requested.as_deref())
        }
        Frame::Bytes(_) => Ok(Vec::new()),
    }
}

/// `finish` (openai-images.ts:257-272).
pub fn finish(state: &State) -> Result<Vec<ImageEvent>, AiError> {
    if state.completed == 0 {
        return Err(identity().incomplete());
    }
    let mut openai = std::collections::BTreeMap::new();
    openai.insert("outputFormat".to_string(), serde_json::json!(state.format));
    openai.insert("size".to_string(), serde_json::json!(state.size));
    openai.insert("quality".to_string(), serde_json::json!(state.quality));
    openai.insert("background".to_string(), serde_json::json!(state.background));
    let mut metadata = crate::schema::ProviderMetadata::new();
    metadata.insert("openai".to_string(), openai);
    Ok(vec![ImageEvent::Finish {
        usage: state.usage.clone(),
        notices: None,
        provider_metadata: Some(metadata),
    }])
}

/// `requestedFormat` (openai-images.ts:183-187): the format the sent body
/// asked for, which the document frame carries for responses that omit it.
/// The route driver reads it when building [`Frame::Document`] inputs.
#[allow(dead_code)] // consumed by the future media-route driver
fn requested_format(body: &Body) -> Option<String> {
    body.json_field("output_format").and_then(serde_json::Value::as_str).map(str::to_string)
}

/// The protocol object implementing [`StreamProtocol`].
#[derive(Debug, Clone, Copy, Default)]
pub struct OpenAiImages;

impl MediaProtocolBase for OpenAiImages {
    fn identity(&self) -> Identity {
        identity()
    }
    fn unsupported(&self) -> &'static [&'static str] {
        &["aspectRatio", "seed"]
    }
    fn base_url(&self) -> String {
        DEFAULT_BASE_URL.to_string()
    }
}

impl StreamProtocol for OpenAiImages {
    type Request = ImageRequest;
    type Event = ImageEvent;
    type State = State;

    fn body_from(&self, request: &Addressed<ImageRequest>) -> Result<Body, AiError> {
        check_unsupported(&request.request)?;
        body_from(request)
    }

    fn path(&self, request: &Addressed<ImageRequest>) -> String {
        if request.request.images.is_empty() {
            PATH.to_string()
        } else {
            EDIT_PATH.to_string()
        }
    }

    fn framing(&self, request: &Addressed<ImageRequest>, _body: &Body) -> Framing {
        if request.mode == Mode::Stream {
            Framing::Sse
        } else {
            Framing::Document
        }
    }

    fn finish(
        &self,
        state: &Self::State,
        _context: &ResponseContext<ImageRequest>,
    ) -> Result<Vec<ImageEvent>, AiError> {
        finish(state)
    }

    fn step(&self, state: &mut Self::State, frame: &Frame) -> Result<Vec<ImageEvent>, AiError> {
        step(state, frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::model::{collect_image_response, CollectedResponse};
    use serde_json::json;

    fn request(model: &str, prompt: &str) -> ImageRequest {
        ImageRequest { model: model.into(), prompt: prompt.into(), ..Default::default() }
    }

    #[test]
    fn generate_lowers_a_json_body() {
        let body =
            body_from(&Addressed::new(request("gpt-image-1", "a cat"), Mode::Generate)).unwrap();
        let Body::Json { value, .. } = body else { panic!("json body") };
        assert_eq!(value["model"], json!("gpt-image-1"));
        assert_eq!(value["prompt"], json!("a cat"));
        assert_eq!(value.get("n"), None);
    }

    #[test]
    fn stream_mode_adds_partials_and_rejects_n() {
        let mut stream_request = request("gpt-image-1", "x");
        stream_request.n = Some(2);
        let error = body_from(&Addressed::new(stream_request, Mode::Stream)).unwrap_err();
        assert!(error.message().contains("streams one image"));

        let body = body_from(&Addressed::new(request("gpt-image-1", "x"), Mode::Stream)).unwrap();
        let Body::Json { value, .. } = body else { panic!() };
        assert_eq!(value["stream"], json!(true));
        assert_eq!(value["partial_images"], json!(2));
    }

    #[test]
    fn native_options_rename_camel_case_keys() {
        let mut options = crate::schema::ProviderOptions::new();
        options.insert("quality".to_string(), json!("high"));
        options.insert("outputCompression".to_string(), json!(80));
        options.insert("partialImages".to_string(), json!(0));
        let mut image_request = request("gpt-image-1", "x");
        image_request.provider_options = Some(options);
        let body = body_from(&Addressed::new(image_request, Mode::Generate)).unwrap();
        let Body::Json { value, .. } = body else { panic!() };
        assert_eq!(value["quality"], json!("high"));
        assert_eq!(value["output_compression"], json!(80));
        assert_eq!(value.get("partial_images"), None);
    }

    #[test]
    fn inline_edit_sources_lower_to_multipart() {
        let mut image_request = request("gpt-image-1", "edit this");
        image_request.images = vec![media::base64("AQID", "image/png")];
        let body = body_from(&Addressed::new(image_request, Mode::Generate)).unwrap();
        let Body::Multipart(form) = body else { panic!("multipart") };
        assert_eq!(form.fields_json()["model"], json!("gpt-image-1"));
        assert_eq!(form.fields_json()["prompt"], json!("edit this"));
        assert_eq!(form.files(), vec![("image[]", "image-0", "image/png")]);
    }

    #[test]
    fn url_edit_sources_lower_to_the_json_edits_body() {
        let mut image_request = request("gpt-image-1", "edit this");
        image_request.images = vec![media::url("https://x/a.png", Some("image/png".into()), None)];
        let body = body_from(&Addressed::new(image_request, Mode::Generate)).unwrap();
        let Body::Json { value, .. } = body else { panic!("json") };
        assert_eq!(value["images"], json!([{ "image_url": "https://x/a.png" }]));
    }

    #[test]
    fn mask_without_images_fails_invalid_request() {
        let mut image_request = request("gpt-image-1", "x");
        image_request.mask = Some(media::base64("AQID", "image/png"));
        let error = body_from(&Addressed::new(image_request, Mode::Generate)).unwrap_err();
        assert_eq!(error.message(), "An OpenAI image mask requires at least one input image");
    }

    #[test]
    fn document_decodes_images_and_requested_format_fallback() {
        let document = r#"{
            "data": [{"b64_json": "AQID"}],
            "output_format": "webp",
            "size": "1024x768",
            "usage": {"input_tokens": 10, "total_tokens": 12}
        }"#;
        let mut state = State::default();
        let events = on_document(&mut state, document, None).unwrap();
        assert_eq!(events.len(), 1);
        let ImageEvent::Image { image, .. } = &events[0] else { panic!() };
        assert_eq!(image.media_type, "image/webp");
        assert_eq!(image.info.as_ref().unwrap().width, Some(1024.0));
        assert_eq!(image.info.as_ref().unwrap().height, Some(768.0));
        let finish_events = finish(&state).unwrap();
        let ImageEvent::Finish { usage, provider_metadata, .. } = &finish_events[0] else {
            panic!()
        };
        assert_eq!(
            usage.as_ref().unwrap(),
            &MediaUsage::Tokens {
                input: Some(10.0),
                output: None,
                total: Some(12.0),
                details: Some(
                    json!({"input_tokens": 10, "total_tokens": 12}).as_object().unwrap().clone()
                ),
            }
        );
        assert_eq!(provider_metadata.as_ref().unwrap()["openai"]["outputFormat"], json!("webp"));

        // Requested-format fallback when the document omits it.
        let mut state = State::default();
        let events =
            on_document(&mut state, r#"{"data": [{"b64_json": "AQID"}]}"#, Some("jpeg")).unwrap();
        let ImageEvent::Image { image, .. } = &events[0] else { panic!() };
        assert_eq!(image.media_type, "image/jpeg");
    }

    #[test]
    fn empty_document_fails_with_route_tag() {
        let mut state = State::default();
        let error = on_document(&mut state, r#"{"data": []}"#, None).unwrap_err();
        assert!(error.message().contains("returned no images"));
        match error.reason {
            crate::schema::AiErrorReason::InvalidProviderOutput { route: Some(route), .. } => {
                assert_eq!(route, ID);
            }
            other => panic!("wrong reason {other:?}"),
        }
    }

    #[test]
    fn sse_events_partial_then_completed() {
        let mut state = State::default();
        let events = on_event(&mut state, r#"{"type":"image_generation.partial_image","b64_json":"AQID","partial_image_index":0,"output_format":"png"}"#).unwrap();
        assert!(matches!(&events[0], ImageEvent::ImagePartial { index: 0.0, .. }));
        let events = on_event(
            &mut state,
            r#"{"type":"image_generation.completed","b64_json":"AQID","output_format":"png","size":"512x512","usage":{"input_tokens":1}}"#,
        )
        .unwrap();
        let ImageEvent::Image { index, .. } = &events[0] else { panic!() };
        assert_eq!(*index, 0.0);
        assert_eq!(state.completed, 1);
        // collectResponse runs over the whole event stream, not just finish.
        let all = [
            vec![ImageEvent::Image { index: 0.0, image: media::base64("AQID", "image/png") }],
            finish(&state).unwrap(),
        ]
        .concat();
        let CollectedResponse::Image(response) = collect_image_response(&all) else { panic!() };
        assert_eq!(response.images.len(), 1);
    }

    #[test]
    fn unsupported_fields_fail_typed_before_lowering() {
        let mut image_request = request("gpt-image-1", "x");
        image_request.aspect_ratio = Some("16:9".into());
        image_request.seed = Some(1.0);
        let error = check_unsupported(&image_request).unwrap_err();
        assert!(error.message().contains("does not support aspectRatio, seed"));
        assert!(matches!(error.reason, crate::schema::AiErrorReason::UnsupportedOperation { .. }));
    }
}
