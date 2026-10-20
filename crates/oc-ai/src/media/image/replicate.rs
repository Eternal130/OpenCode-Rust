//! `replicate-images` (port of `protocols/replicate-images.ts`). Queued
//! protocol over Replicate predictions; outputs are kept for one hour,
//! and data-URL inputs are limited to 256 KB.

use std::collections::BTreeMap;

use crate::media::body::{merge_records, Body};
use crate::media::model::{ImageRequest, ImageResponse, PollContext, Snapshot, Started, Status};
use crate::media::protocol::{
    status as table_status, Identity, MediaProtocolBase, QueuedProtocol, TextOutput,
};
use crate::media::utils::media_reference;
use crate::schema::media::{self, Asset};
use crate::schema::{AiError, MediaUsage};

pub const ID: &str = "replicate-images";
pub const DEFAULT_BASE_URL: &str = "https://api.replicate.com";
/// Output URLs are valid for one hour (replicate-images.ts:13).
pub const OUTPUT_RETENTION_MS: f64 = 60.0 * 60.0 * 1000.0;
/// Data-URL inputs are limited to 256 KB (replicate-images.ts:14).
pub const MAX_DATA_URL_BYTES: f64 = 256.0 * 1024.0;

pub fn identity() -> Identity {
    Identity::new(ID, "Replicate", "replicate")
}

/// The serializable token (replicate-images.ts:29).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Token {
    pub id: String,
    pub get_url: String,
    pub cancel_url: String,
}

fn status_table() -> BTreeMap<&'static str, Status> {
    [
        ("starting", Status::Queued),
        ("processing", Status::Running),
        ("succeeded", Status::Completed),
        ("failed", Status::Failed),
        ("canceled", Status::Cancelled),
        // The prediction hit its `Cancel-After` deadline before it started
        // running.
        ("aborted", Status::Expired),
    ]
    .into_iter()
    .collect()
}

/// Official models run by `owner/name`; anything else (`owner/name:
/// version` or a bare version id) is a pinned version
/// (replicate-images.ts:61).
#[must_use]
pub fn is_official(model: &str) -> bool {
    let mut parts = model.split('/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(owner), Some(name), None) => {
            !owner.is_empty() && !name.is_empty() && !model.contains(':')
        }
        _ => false,
    }
}

fn inline_size(asset: &Asset) -> f64 {
    match &asset.source {
        media::Source::Bytes { data, .. } => data.0.len() as f64,
        media::Source::Base64 { data, .. } => data.len() as f64 * 0.75,
        _ => 0.0,
    }
}

/// `fileInput` (replicate-images.ts:69-77).
fn file_input(asset: &Asset) -> Result<String, AiError> {
    if inline_size(asset) > MAX_DATA_URL_BYTES {
        return Err(crate::protocols::shared::invalid_request(
            "Replicate data URL inputs are limited to 256 KB; pass a larger file by https URL",
        ));
    }
    media_reference(asset, None, "Replicate").map(|(_, value)| value)
}

/// `fromRequest` (replicate-images.ts:86-95): model-defined `input` fields
/// under the model's own names; `Media.Asset` values lower to URLs or data
/// URLs.
pub fn body_from(request: &ImageRequest) -> Result<Body, AiError> {
    let mut input = serde_json::Map::new();
    input.insert("prompt".to_string(), serde_json::json!(request.prompt));
    if let Some(options) = &request.provider_options {
        for (key, value) in options {
            let lowered = lower_input(value)?;
            input.insert(key.clone(), lowered);
        }
    }
    let base = if is_official(&request.model) {
        serde_json::json!({ "input": input })
    } else {
        serde_json::json!({ "version": request.model, "input": input })
    };
    let merged = merge_records(&[
        Some(
            serde_json::from_value::<crate::schema::options::JsonRecord>(base).unwrap_or_default(),
        )
        .as_ref(),
        request.http.as_ref().and_then(|http| http.body.as_ref()),
    ])
    .unwrap_or_default();
    Ok(Body::json(merged))
}

/// `inputValue` (replicate-images.ts:79-84): assets lower inside arrays too.
fn lower_input(value: &serde_json::Value) -> Result<serde_json::Value, AiError> {
    if let Some(asset) = asset_of(value) {
        return file_input(&asset).map(serde_json::Value::String);
    }
    if let Some(items) = value.as_array() {
        if items.iter().any(|item| asset_of(item).is_some()) {
            let mut out = Vec::new();
            for item in items {
                out.push(match asset_of(item) {
                    Some(asset) => serde_json::Value::String(file_input(&asset)?),
                    None => item.clone(),
                });
            }
            return Ok(serde_json::Value::Array(out));
        }
    }
    Ok(value.clone())
}

/// TS checks `value instanceof Media.Asset`; the Rust wire form of a
/// passed-through asset is a serialized `Asset` object (`source.type`).
fn asset_of(value: &serde_json::Value) -> Option<Asset> {
    value.get("source")?.get("type")?;
    serde_json::from_value::<Asset>(value.clone()).ok()
}

fn decode_prediction(output: &TextOutput) -> Result<serde_json::Value, AiError> {
    output.decode_json(output.body)
}

/// `decodeStart` (replicate-images.ts:103-113): `Prefer: wait` can return
/// an already-finished prediction.
pub fn decode_start(output: &TextOutput) -> Result<Started<Token>, AiError> {
    let prediction = decode_prediction(output)?;
    let id =
        prediction.get("id").and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
    let raw = prediction.get("status").and_then(serde_json::Value::as_str).unwrap_or_default();
    let status = table_status(&status_table(), raw, output)?;
    Ok(Started {
        snapshot: Snapshot { id: id.clone(), status, progress: None, position: None },
        token: Token {
            id,
            get_url: prediction
                .pointer("/urls/get")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            cancel_url: prediction
                .pointer("/urls/cancel")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
        },
    })
}

/// `decodeStatus` (replicate-images.ts:115-121).
pub fn decode_status(
    output: &TextOutput,
    context: &PollContext<Token>,
) -> Result<Snapshot, AiError> {
    let prediction = decode_prediction(output)?;
    let raw = prediction.get("status").and_then(serde_json::Value::as_str).unwrap_or_default();
    Ok(Snapshot {
        id: context.token.id.clone(),
        status: table_status(&status_table(), raw, output)?,
        progress: None,
        position: None,
    })
}

/// `decodeResult` (replicate-images.ts:123-151).
pub fn decode_result(
    output: &TextOutput,
    context: &PollContext<Token>,
) -> Result<ImageResponse, AiError> {
    let prediction = decode_prediction(output)?;
    let raw = prediction.get("status").and_then(serde_json::Value::as_str).unwrap_or_default();
    let status = table_status(&status_table(), raw, output)?;
    if matches!(status, Status::Failed | Status::Cancelled | Status::Expired) {
        let error = prediction.get("error").and_then(serde_json::Value::as_str);
        return Err(output.ended(
            status,
            format!(
                "Replicate prediction {} {raw}{}",
                context.token.id,
                error.map(|error| format!(": {error}")).unwrap_or_default()
            ),
            crate::media::protocol::Failure::ProviderInternal,
        ));
    }
    if status != Status::Completed {
        return Err(output.pending(&context.token.id));
    }
    if prediction.get("data_removed").and_then(serde_json::Value::as_bool) == Some(true) {
        return Err(output.ended(
            Status::Expired,
            format!("Replicate removed the output of prediction {}", context.token.id),
            crate::media::protocol::Failure::ProviderInternal,
        ));
    }
    let Some(prediction_output) = prediction.get("output").filter(|value| !value.is_null()) else {
        return Err(output.invalid(format!(
            "Replicate prediction {} returned output that is not image URLs",
            context.token.id
        )));
    };
    let urls: Vec<String> = match prediction_output {
        serde_json::Value::String(url) => vec![url.clone()],
        serde_json::Value::Array(items) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect::<Vec<String>>(),
        _ => {
            return Err(output.invalid(format!(
                "Replicate prediction {} returned output that is not image URLs",
                context.token.id
            )));
        }
    };
    if urls.is_empty() {
        return Err(
            output.invalid(format!("Replicate prediction {} returned no images", context.token.id))
        );
    }
    let predict_time =
        prediction.pointer("/metrics/predict_time").and_then(serde_json::Value::as_f64);
    let mut replicate = std::collections::BTreeMap::new();
    replicate.insert(
        "predictionId".to_string(),
        serde_json::json!(prediction.get("id").cloned().unwrap_or_default()),
    );
    let mut metadata = crate::schema::ProviderMetadata::new();
    metadata.insert("replicate".to_string(), replicate);
    Ok(ImageResponse {
        images: urls
            .into_iter()
            .map(|url| crate::media::protocol::expiring_url(url, OUTPUT_RETENTION_MS, None, None))
            .collect(),
        usage: predict_time.map(|seconds| MediaUsage::Compute { seconds }),
        notices: None,
        provider_metadata: Some(metadata),
    })
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ReplicateImages;

impl MediaProtocolBase for ReplicateImages {
    fn identity(&self) -> Identity {
        identity()
    }
    fn unsupported(&self) -> &'static [&'static str] {
        &["images", "mask", "n", "size", "aspectRatio", "seed", "format"]
    }
    fn base_url(&self) -> String {
        DEFAULT_BASE_URL.to_string()
    }
}

impl QueuedProtocol for ReplicateImages {
    type Request = ImageRequest;
    type Response = ImageResponse;
    type Token = Token;

    fn body_from(&self, request: &ImageRequest) -> Result<Body, AiError> {
        body_from(request)
    }

    fn start_path(&self, request: &ImageRequest) -> String {
        if is_official(&request.model) {
            format!("/v1/models/{}/predictions", request.model)
        } else {
            "/v1/predictions".to_string()
        }
    }

    fn decode_start(&self, output: &TextOutput) -> Result<Started<Token>, AiError> {
        decode_start(output)
    }

    fn status_path(&self, token: &Token) -> String {
        token.get_url.clone()
    }

    fn decode_status(
        &self,
        output: &TextOutput,
        context: &PollContext<Token>,
    ) -> Result<Snapshot, AiError> {
        decode_status(output, context)
    }

    fn result_path(&self, token: &Token) -> String {
        token.get_url.clone()
    }

    fn decode_result(
        &self,
        output: &TextOutput,
        context: &PollContext<Token>,
    ) -> Result<ImageResponse, AiError> {
        decode_result(output, context)
    }

    fn cancel(&self) -> Option<crate::media::protocol::CancelSpec> {
        Some(crate::media::protocol::CancelSpec { method: "POST", active_only: false })
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

    fn cancel_path(&self, token: &Token) -> String {
        token.cancel_url.clone()
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
    fn official_vs_versioned_models() {
        assert!(is_official("black-forest-labs/flux-schnell"));
        assert!(!is_official("black-forest-labs/flux-schnell:abc123"));
        assert!(!is_official("abc123"));
    }

    #[test]
    fn official_model_lowers_input_only() {
        let Body::Json { value, .. } = body_from(&request("owner/name")).unwrap() else { panic!() };
        assert_eq!(value["input"]["prompt"], json!("a cat"));
        assert!(value.get("version").is_none());
    }

    #[test]
    fn versioned_model_pins_the_version() {
        let Body::Json { value, .. } = body_from(&request("owner/name:abc123")).unwrap() else {
            panic!()
        };
        assert_eq!(value["version"], json!("owner/name:abc123"));
    }

    #[test]
    fn asset_options_lower_to_urls() {
        let mut options = crate::schema::ProviderOptions::new();
        options.insert(
            "image".to_string(),
            serde_json::to_value(media::url("https://x/a.png", None, None)).unwrap(),
        );
        options.insert(
            "refs".to_string(),
            json!([
                serde_json::to_value(media::url("https://x/b.png", None, None)).unwrap(),
                "plain"
            ]),
        );
        let mut image_request = request("owner/name");
        image_request.provider_options = Some(options);
        let Body::Json { value, .. } = body_from(&image_request).unwrap() else { panic!() };
        assert_eq!(value["input"]["image"], json!("https://x/a.png"));
        assert_eq!(value["input"]["refs"], json!(["https://x/b.png", "plain"]));
    }

    #[test]
    fn decode_result_expires_urls_and_reports_compute() {
        let identity = identity();
        let auth = BTreeMap::new();
        let token = Token {
            id: "p1".into(),
            get_url: "https://get".into(),
            cancel_url: "https://cancel".into(),
        };
        let context = PollContext { token: &token, auth: &auth };
        let output = TextOutput {
            identity: &identity,
            body: r#"{"id":"p1","status":"succeeded","output":["https://x/1.png"],"metrics":{"predict_time":4.2}}"#,
            http: None,
        };
        let response = decode_result(&output, &context).unwrap();
        assert_eq!(response.images.len(), 1);
        assert!(response.images[0].expires_at.is_some());
        assert_eq!(response.usage, Some(MediaUsage::Compute { seconds: 4.2 }));

        let failed = TextOutput {
            identity: &identity,
            body: r#"{"id":"p1","status":"failed","error":"OOM"}"#,
            http: None,
        };
        let error = decode_result(&failed, &context).unwrap_err();
        assert!(error.message().contains("failed: OOM"));
        assert!(matches!(error.reason, crate::schema::AiErrorReason::ProviderInternal { .. }));

        let removed = TextOutput {
            identity: &identity,
            body: r#"{"id":"p1","status":"succeeded","data_removed":true}"#,
            http: None,
        };
        let error = decode_result(&removed, &context).unwrap_err();
        assert!(error.message().contains("removed the output"));
    }
}
