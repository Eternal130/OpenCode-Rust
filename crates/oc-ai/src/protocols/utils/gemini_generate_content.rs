//! Shared Gemini generateContent utilities (port of the chat-protocol
//! slice of `src/protocols/utils/gemini-generate-content.ts`).
//!
//! The remaining helpers in that TS module (`track`/`blocked`/`withheld`/
//! `notices`/`usage`/`frames`/`chunk`) serve the media protocols
//! (images/video/speech/transcription), which are descoped per ai-map §10;
//! they land with the media lanes. The pieces here are exactly what the
//! gemini chat protocol consumes: the content-filter finish-reason set,
//! the input-media lowering, and the model-embedding request path.

use serde_json::{json, Value};

use crate::schema::media::Asset;
use crate::schema::{AiError, ProviderId};

/// Gemini's provider id (Files API references are only forwarded for refs
/// owned by this provider).
pub const PROVIDER: &str = "google";

/// Finish reasons for which Gemini stops output on safety or policy
/// grounds.
const CONTENT_FILTER_REASONS: [&str; 10] = [
    "IMAGE_SAFETY",
    "RECITATION",
    "SAFETY",
    "BLOCKLIST",
    "PROHIBITED_CONTENT",
    "SPII",
    "MODEL_ARMOR",
    "IMAGE_PROHIBITED_CONTENT",
    "IMAGE_RECITATION",
    "LANGUAGE",
];

/// Whether a finish reason stops output on safety or policy grounds.
#[must_use]
pub fn content_filtered(finish_reason: Option<&str>) -> bool {
    finish_reason.is_some_and(|reason| CONTENT_FILTER_REASONS.contains(&reason))
}

/// Request path for one model: `streamGenerateContent?alt=sse` when
/// streaming, `generateContent` for a single document (port of `path`).
#[must_use]
pub fn path(model: &str, stream: bool) -> String {
    if stream {
        format!("/models/{model}:streamGenerateContent?alt=sse")
    } else {
        format!("/models/{model}:generateContent")
    }
}

/// This provider's own file handle for `ref` assets; refs from other
/// providers are never forwarded (port of `MediaInput.refID`).
fn ref_id(asset: &Asset) -> Option<&str> {
    match &asset.source {
        crate::schema::media::Source::Ref { provider, id, .. }
            if *provider == ProviderId::new(PROVIDER) =>
        {
            Some(id)
        }
        _ => None,
    }
}

/// `asset.source.type` tag for error messages.
fn source_kind(asset: &Asset) -> &'static str {
    match &asset.source {
        crate::schema::media::Source::Bytes { .. } => "bytes",
        crate::schema::media::Source::Base64 { .. } => "base64",
        crate::schema::media::Source::Url { .. } => "url",
        crate::schema::media::Source::Ref { .. } => "ref",
    }
}

/// Lower one input media asset: a Gemini Files API reference when the asset
/// is a google-owned `ref`, inline base64 otherwise. Gemini does not fetch
/// public URLs (port of `mediaPart`).
///
/// Returns the wire part object — `{fileData:{mimeType, fileUri}}` or
/// `{inlineData:{mimeType, data}}`.
pub fn media_part(route: &str, asset: &Asset) -> Result<Value, AiError> {
    if let Some(file_uri) = ref_id(asset) {
        return Ok(json!({
            "fileData": { "mimeType": asset.media_type, "fileUri": file_uri }
        }));
    }
    let inline = asset.inline().ok_or_else(|| {
        crate::protocols::shared::invalid_request(format!(
            "{route} requires inline media (bytes or base64); {} sources must be materialized first",
            source_kind(asset)
        ))
    })?;
    Ok(json!({ "inlineData": { "mimeType": inline.mime, "data": inline.base64 } }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::media;

    #[test]
    fn content_filter_reasons_match_the_google_set() {
        for reason in
            ["SAFETY", "RECITATION", "BLOCKLIST", "MODEL_ARMOR", "IMAGE_RECITATION", "LANGUAGE"]
        {
            assert!(content_filtered(Some(reason)), "{reason}");
        }
        assert!(!content_filtered(Some("STOP")));
        assert!(!content_filtered(Some("MAX_TOKENS")));
        assert!(!content_filtered(None));
    }

    #[test]
    fn paths_embed_the_model_and_mode() {
        assert_eq!(
            path("gemini-2.5-flash", true),
            "/models/gemini-2.5-flash:streamGenerateContent?alt=sse"
        );
        assert_eq!(path("gemini-2.5-flash", false), "/models/gemini-2.5-flash:generateContent");
    }

    #[test]
    fn media_parts_inline_base64_assets() {
        let asset = media::base64("AAECAw==", "image/png");
        assert_eq!(
            media_part("Gemini", &asset).unwrap(),
            serde_json::json!({ "inlineData": { "mimeType": "image/png", "data": "AAECAw==" } })
        );
    }

    #[test]
    fn media_parts_forward_google_file_references() {
        let asset = media::reference("google", "files/abc-123", Some("image/png".into()));
        assert_eq!(
            media_part("Gemini", &asset).unwrap(),
            serde_json::json!({ "fileData": { "mimeType": "image/png", "fileUri": "files/abc-123" } })
        );
        // other providers' refs are never forwarded
        let foreign = media::reference("openai", "file_123", Some("image/png".into()));
        let error = media_part("Gemini", &foreign).unwrap_err();
        assert!(error.message().contains("requires inline media"));
    }

    #[test]
    fn url_assets_fail_typed_until_materialized() {
        let asset = media::url("https://example.test/a.png", Some("image/png".into()), None);
        let error = media_part("Gemini", &asset).unwrap_err();
        assert!(error.message().contains("url sources must be materialized first"), "{}", error);
    }
}
