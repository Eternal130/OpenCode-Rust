//! `google-images` (port of `protocols/google-images.ts`).
//!
//! Inline protocol over Gemini `generateContent` with
//! `responseModalities: ["IMAGE"]`; thought parts are drafts and only
//! non-thought inline data is a final image.

use crate::media::body::{merge_records, Body};
use crate::media::model::{image_field_present, DecodeContext, ImageRequest, ImageResponse};
use crate::media::protocol::{Identity, InlineProtocol, MediaProtocolBase, TextOutput};
use crate::media::utils::{self, gemini};
use crate::schema::media::{Notice, NoticeType};
use crate::schema::{AiError, MediaUsage};

pub const ID: &str = "google-images";
pub const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

pub fn identity() -> Identity {
    Identity::new(ID, "Google Images", "google")
}

/// `generationConfig` (google-images.ts:84-99).
fn generation_config(request: &ImageRequest) -> crate::schema::ProviderOptions {
    let mut native = request.provider_options.clone().unwrap_or_default();
    let image_size = native.remove("imageSize");
    let thinking_level = native.remove("thinkingLevel");
    let include_thoughts = native.remove("includeThoughts");

    let mut image_config = serde_json::Map::new();
    if let Some(ratio) = &request.aspect_ratio {
        image_config.insert("aspectRatio".to_string(), serde_json::json!(ratio));
    }
    if let Some(size) = image_size {
        image_config.insert("imageSize".to_string(), size);
    }
    let mut thinking_config = serde_json::Map::new();
    if let Some(level) = thinking_level {
        thinking_config.insert("thinkingLevel".to_string(), level);
    }
    if let Some(thoughts) = include_thoughts {
        thinking_config.insert("includeThoughts".to_string(), thoughts);
    }

    let base: Vec<(String, serde_json::Value)> = vec![
        ("responseModalities".to_string(), serde_json::json!(["IMAGE"])),
        (
            "imageConfig".to_string(),
            if image_config.is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::Value::Object(image_config.clone())
            },
        ),
        ("seed".to_string(), crate::media::body::json_opt(request.seed)),
        (
            "thinkingConfig".to_string(),
            if thinking_config.is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::Value::Object(thinking_config.clone())
            },
        ),
    ];
    let base_record: serde_json::Map<String, serde_json::Value> =
        base.into_iter().filter(|(_, value)| !value.is_null()).collect();
    merge_records(&[Some(&base_record), Some(&native)]).unwrap_or_else(|| {
        let mut fallback = serde_json::Map::new();
        fallback.insert("responseModalities".to_string(), serde_json::json!(["IMAGE"]));
        fallback
    })
}

/// `fromRequest` (google-images.ts:101-119).
pub fn body_from(request: &ImageRequest) -> Result<Body, AiError> {
    if let Some(n) = request.n {
        if n > 1 {
            return Err(identity().unsupported(
                "media.n",
                "Google Images generates one image per request; call it once per image instead of n={n}",
            ));
        }
    }
    let mut parts: Vec<serde_json::Value> = vec![serde_json::json!({ "text": request.prompt })];
    for image in &request.images {
        parts.push(gemini::media_part("Google Images", image)?);
    }
    let contents = serde_json::json!([{ "role": "user", "parts": parts }]);
    let config =
        serde_json::to_value(generation_config(request)).unwrap_or(serde_json::Value::Null);
    let merged = merge_records(&[
        Some(
            &serde_json::from_value::<serde_json::Map<String, serde_json::Value>>(
                serde_json::json!({ "contents": contents, "generationConfig": config }),
            )
            .unwrap_or_default(),
        ),
        request.http.as_ref().and_then(|http| http.body.as_ref()),
    ])
    .unwrap_or_default();
    Ok(Body::json(merged))
}

/// The route's unsupported-field rejection (`mask`, `size`, `format`).
pub fn check_unsupported(request: &ImageRequest) -> Result<(), AiError> {
    let present: Vec<&str> = ["mask", "size", "format"]
        .into_iter()
        .filter(|field| image_field_present(request, field))
        .collect();
    identity().reject_unsupported(&present).map_or(Ok(()), Err)
}

/// `decodeResponse` (google-images.ts:127-259).
pub fn decode(
    output: &TextOutput,
    context: &DecodeContext<ImageRequest>,
) -> Result<ImageResponse, AiError> {
    let decoded = output.decode_json(output.body)?;
    let candidates = decoded
        .get("candidates")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();

    // Thought parts are drafts; only non-thought inline data is a final
    // image (google-images.ts:151-166).
    let mut images = Vec::new();
    for (candidate_index, candidate) in candidates.iter().enumerate() {
        let parts = candidate
            .pointer("/content/parts")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        for (part_index, part) in parts.iter().enumerate() {
            let Some(inline) = part.get("inlineData") else { continue };
            if part.get("thought").and_then(serde_json::Value::as_bool) == Some(true) {
                continue;
            }
            let mut google = std::collections::BTreeMap::new();
            google.insert(
                "candidateIndex".to_string(),
                candidate
                    .get("index")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!(candidate_index)),
            );
            google.insert("partIndex".to_string(), serde_json::json!(part_index));
            google.insert(
                "finishReason".to_string(),
                candidate.get("finishReason").cloned().unwrap_or(serde_json::Value::Null),
            );
            google.insert(
                "safetyRatings".to_string(),
                candidate.get("safetyRatings").cloned().unwrap_or(serde_json::Value::Null),
            );
            google.insert(
                "citationMetadata".to_string(),
                candidate.get("citationMetadata").cloned().unwrap_or(serde_json::Value::Null),
            );
            google.insert(
                "groundingMetadata".to_string(),
                candidate.get("groundingMetadata").cloned().unwrap_or(serde_json::Value::Null),
            );
            google.insert(
                "thoughtSignature".to_string(),
                part.get("thoughtSignature").cloned().unwrap_or(serde_json::Value::Null),
            );
            let mut metadata = crate::schema::ProviderMetadata::new();
            metadata.insert("google".to_string(), google);
            images.push(utils::decoded_asset(
                |message| output.invalid(message),
                &format!("Google Images candidate {candidate_index} part {part_index}"),
                inline.get("data").and_then(serde_json::Value::as_str).unwrap_or_default(),
                inline.get("mimeType").and_then(serde_json::Value::as_str),
                None,
                Some(metadata),
            )?);
        }
    }
    if images.is_empty() {
        let reasons: Vec<String> = candidates
            .iter()
            .filter_map(|candidate| {
                candidate.get("finishReason").and_then(serde_json::Value::as_str)
            })
            .map(str::to_string)
            .collect();
        let suffix = if reasons.is_empty() {
            String::new()
        } else {
            format!(" (finish reasons: {})", reasons.join(", "))
        };
        return Err(output.invalid(format!(
            "Google Images returned no final images{suffix}; inspect body for prompt feedback and candidate details"
        )));
    }

    // Candidates that stopped for a safety or policy reason are partial
    // results, not a silent drop (google-images.ts:198-229).
    let mut notices: Vec<Notice> = Vec::new();
    if decoded.get("promptFeedback").is_some() {
        let mut google = std::collections::BTreeMap::new();
        google.insert(
            "promptFeedback".to_string(),
            decoded.get("promptFeedback").cloned().unwrap_or_default(),
        );
        let mut metadata = crate::schema::ProviderMetadata::new();
        metadata.insert("google".to_string(), google);
        notices.push(Notice {
            kind: NoticeType::Filtered,
            message: "Google Images reported prompt feedback".to_string(),
            provider_metadata: Some(metadata),
        });
    }
    for (index, candidate) in candidates.iter().enumerate() {
        let Some(reason) = candidate.get("finishReason").and_then(serde_json::Value::as_str) else {
            continue;
        };
        if reason == "STOP" {
            continue;
        }
        let finish_message = candidate.get("finishMessage").and_then(serde_json::Value::as_str);
        let mut google = std::collections::BTreeMap::new();
        google.insert(
            "candidateIndex".to_string(),
            candidate.get("index").cloned().unwrap_or_else(|| serde_json::json!(index)),
        );
        google.insert("finishReason".to_string(), serde_json::json!(reason));
        google.insert("finishMessage".to_string(), serde_json::json!(finish_message));
        google.insert(
            "safetyRatings".to_string(),
            candidate.get("safetyRatings").cloned().unwrap_or_default(),
        );
        let mut metadata = crate::schema::ProviderMetadata::new();
        metadata.insert("google".to_string(), google);
        notices.push(Notice {
            kind: NoticeType::Filtered,
            message: format!(
                "Google Images candidate {} finished with {reason}{}",
                candidate.get("index").and_then(serde_json::Value::as_f64).unwrap_or(index as f64),
                finish_message.map(|message| format!(": {message}")).unwrap_or_default()
            ),
            provider_metadata: Some(metadata),
        });
    }

    let usage = decoded.get("usageMetadata");
    let (usage_value, output_tokens) = match usage {
        Some(value) if !value.is_null() => {
            let prompt = value.get("promptTokenCount").and_then(serde_json::Value::as_f64);
            let candidates_count =
                value.get("candidatesTokenCount").and_then(serde_json::Value::as_f64);
            let thoughts = value.get("thoughtsTokenCount").and_then(serde_json::Value::as_f64);
            let output = candidates_count.map(|count| count + thoughts.unwrap_or(0.0));
            (
                Some(MediaUsage::Tokens {
                    input: prompt,
                    output,
                    total: crate::protocols::shared::total_tokens(
                        prompt,
                        output,
                        value.get("totalTokenCount").and_then(serde_json::Value::as_f64),
                    ),
                    details: Some(
                        serde_json::from_value::<serde_json::Map<String, serde_json::Value>>(
                            value.clone(),
                        )
                        .unwrap_or_default(),
                    ),
                }),
                output,
            )
        }
        _ => (None, None),
    };
    let mut google = std::collections::BTreeMap::new();
    google.insert(
        "modelVersion".to_string(),
        decoded.get("modelVersion").cloned().unwrap_or_default(),
    );
    google.insert("responseId".to_string(), decoded.get("responseId").cloned().unwrap_or_default());
    google.insert(
        "promptFeedback".to_string(),
        decoded.get("promptFeedback").cloned().unwrap_or_default(),
    );
    let mut metadata = crate::schema::ProviderMetadata::new();
    metadata.insert("google".to_string(), google);
    let _ = context;
    let _ = output_tokens;
    Ok(ImageResponse {
        images,
        notices: if notices.is_empty() { None } else { Some(notices) },
        usage: usage_value,
        provider_metadata: Some(metadata),
    })
}

/// The protocol object implementing [`InlineProtocol`].
#[derive(Debug, Clone, Copy, Default)]
pub struct GoogleImages;

impl MediaProtocolBase for GoogleImages {
    fn identity(&self) -> Identity {
        identity()
    }
    fn unsupported(&self) -> &'static [&'static str] {
        &["mask", "size", "format"]
    }
    fn base_url(&self) -> String {
        DEFAULT_BASE_URL.to_string()
    }
}

impl InlineProtocol for GoogleImages {
    type Request = ImageRequest;
    type Response = ImageResponse;

    fn body_from(&self, request: &ImageRequest) -> Result<Body, AiError> {
        check_unsupported(request)?;
        body_from(request)
    }

    fn path(&self, request: &ImageRequest) -> String {
        format!("/models/{}:generateContent", request.model)
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
        ImageRequest {
            model: "gemini-3-pro-image-preview".into(),
            prompt: "a cat".into(),
            ..Default::default()
        }
    }

    #[test]
    fn lowers_contents_with_response_modalities() {
        let body = body_from(&request()).unwrap();
        let Body::Json { value, .. } = body else { panic!() };
        assert_eq!(value["contents"][0]["parts"][0]["text"], json!("a cat"));
        assert_eq!(value["generationConfig"]["responseModalities"], json!(["IMAGE"]));
        assert!(value.get("model").is_none());
    }

    #[test]
    fn image_config_and_seed_pass_through() {
        let mut image_request = request();
        image_request.aspect_ratio = Some("16:9".into());
        image_request.seed = Some(7.0);
        let mut options = crate::schema::ProviderOptions::new();
        options.insert("imageSize".to_string(), json!("2K"));
        options.insert("thinkingLevel".to_string(), json!("LOW"));
        image_request.provider_options = Some(options);
        let body = body_from(&image_request).unwrap();
        let Body::Json { value, .. } = body else { panic!() };
        assert_eq!(
            value["generationConfig"]["imageConfig"],
            json!({ "aspectRatio": "16:9", "imageSize": "2K" })
        );
        assert_eq!(value["generationConfig"]["thinkingConfig"], json!({ "thinkingLevel": "LOW" }));
        assert_eq!(value["generationConfig"]["seed"], json!(7));
    }

    #[test]
    fn rejects_n_gt_one_typed() {
        let mut image_request = request();
        image_request.n = Some(3);
        let error = body_from(&image_request).unwrap_err();
        assert!(matches!(error.reason, crate::schema::AiErrorReason::UnsupportedOperation { .. }));
        assert!(error.message().contains("one image per request"));
    }

    #[test]
    fn decodes_inline_data_and_notices() {
        let document = r#"{
            "candidates": [{
                "index": 0,
                "content": {"parts": [
                    {"inlineData": {"mimeType": "image/png", "data": "AQID"}},
                    {"inlineData": {"mimeType": "image/png", "data": "AAAA"}, "thought": true}
                ]},
                "finishReason": "STOP"
            }],
            "usageMetadata": {"promptTokenCount": 5, "candidatesTokenCount": 100, "thoughtsTokenCount": 20, "totalTokenCount": 125},
            "modelVersion": "gemini-3-pro"
        }"#;
        let identity = identity();
        let output = TextOutput { identity: &identity, body: document, http: None };
        let body = Body::json(Default::default());
        let context = DecodeContext { request: &request(), body: &body };
        let response = decode(&output, &context).unwrap();
        assert_eq!(response.images.len(), 1);
        assert_eq!(response.images[0].media_type, "image/png");
        assert_eq!(
            response.provider_metadata.as_ref().unwrap()["google"]["modelVersion"],
            json!("gemini-3-pro")
        );
        assert_eq!(response.usage.as_ref().unwrap().clone(), MediaUsage::Tokens {
            input: Some(5.0),
            output: Some(120.0),
            total: Some(125.0),
            details: Some(json!({"promptTokenCount": 5, "candidatesTokenCount": 100, "thoughtsTokenCount": 20, "totalTokenCount": 125}).as_object().unwrap().clone()),
        });
    }

    #[test]
    fn no_images_lists_finish_reasons() {
        let document = r#"{"candidates": [{"finishReason": "SAFETY"}]}"#;
        let identity = identity();
        let output = TextOutput { identity: &identity, body: document, http: None };
        let body = Body::json(Default::default());
        let context = DecodeContext { request: &request(), body: &body };
        let error = decode(&output, &context).unwrap_err();
        assert!(error.message().contains("finish reasons: SAFETY"));
    }

    #[test]
    fn unsupported_size_fails_typed() {
        let mut image_request = request();
        image_request.size = Some("1024x768".into());
        let error = check_unsupported(&image_request).unwrap_err();
        assert_eq!(error.message(), "google/google-images does not support size");
    }
}
