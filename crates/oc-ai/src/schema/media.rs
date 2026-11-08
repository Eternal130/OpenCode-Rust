//! `Media.Asset` type only (port of `src/media.ts` with media *protocols*
//! descoped per ai-map §10: ~6.4k LOC of image/video/speech/transcription
//! routes are out of scope for this lane).
//!
//! The asset type stays because messages/events carry media parts, and
//! protocols branch on `source.type` (`requireInlineMedia` / `mediaUrl` /
//! `mediaReference` — see [`crate::protocols::shared`]). Downloading `url`
//! sources runs through the caller-supplied `fetch` seam of
//! [`Asset::bytes`]/[`Asset::materialize`] (the route layer wires its
//! transport there, G-246); `ref` sources fail typed.

use std::collections::BTreeMap;
use std::future::Future;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::errors::{AiError, AiErrorReason, ReasonFields};
use super::ids::ProviderId;
use super::options::ProviderMetadata;

/// Media kind bucket derived from the media type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Image,
    Video,
    Audio,
    Document,
    Other,
}

/// Bucket a media (MIME) type into a [`Kind`] (port of `kindOf`).
#[must_use]
pub fn kind_of(media_type: &str) -> Kind {
    let lower = media_type.to_ascii_lowercase();
    if lower.starts_with("image/") {
        Kind::Image
    } else if lower.starts_with("video/") {
        Kind::Video
    } else if lower.starts_with("audio/") {
        Kind::Audio
    } else if lower == "application/pdf" || lower.starts_with("text/") {
        Kind::Document
    } else {
        Kind::Other
    }
}

/// Container-independent facts about a payload.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channels: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoding: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
}

/// Serializable wire/persistence form of a media asset source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Source {
    Bytes {
        data: BytesPayload,
        media_type: String,
    },
    Base64 {
        data: String,
        media_type: String,
    },
    Url {
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        media_type: Option<String>,
        /// Epoch milliseconds after which the provider no longer serves the URL.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expires_at: Option<f64>,
    },
    /// A provider-side handle: `OpenAI` `file_id`, Gemini file URI, `gs://`, …
    Ref {
        provider: ProviderId,
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        media_type: Option<String>,
    },
}

/// JSON-encoded byte payload (`Uint8Array` on the TS wire becomes a JSON
/// array of byte numbers; base64 sources should use [`Source::Base64`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BytesPayload(pub Vec<u8>);

/// Synchronous view of an inline payload; `url`/`ref` sources carry none.
#[derive(Debug, Clone, PartialEq)]
pub struct Inline {
    pub mime: String,
    pub base64: String,
    pub data_url: String,
}

/// A media asset: serializable source plus derived metadata (port of
/// `Media.Asset`; the effectful byte cache of the ref is a documented
/// divergence — see [`Asset::bytes`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub source: Source,
    /// Declared type, or `application/octet-stream` when the source omits it.
    #[serde(default)]
    pub media_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info: Option<Info>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
    /// Transient download credentials for `url` sources (media.ts:117-118,
    /// 208-213): runtime-only, never part of the serde face — a persisted
    /// asset cannot leak credentials and cannot be downloaded again after
    /// a round-trip; materialize before persisting. G-246.
    #[serde(skip)]
    pub headers: Option<BTreeMap<String, String>>,
}

impl Asset {
    /// Derived kind bucket.
    #[must_use]
    pub fn kind(&self) -> Kind {
        kind_of(&self.media_type)
    }

    /// Inline payload for `bytes`/`base64` sources; `None` for `url`/`ref`.
    #[must_use]
    pub fn inline(&self) -> Option<Inline> {
        match &self.source {
            Source::Bytes { data, media_type } => {
                let base64 = super::super::protocols::shared::encode_base64(&data.0);
                let mime = media_type.to_ascii_lowercase();
                Some(Inline { data_url: format!("data:{mime};base64,{base64}"), mime, base64 })
            }
            Source::Base64 { data, media_type } => {
                let mime = media_type.to_ascii_lowercase();
                Some(Inline {
                    data_url: format!("data:{mime};base64,{data}"),
                    mime,
                    base64: data.clone(),
                })
            }
            _ => None,
        }
    }

    /// The remote URL of a `url` source, if this is one.
    #[must_use]
    pub fn url(&self) -> Option<&str> {
        match &self.source {
            Source::Url { url, .. } => Some(url),
            _ => None,
        }
    }

    /// Decoded payload (media.ts:149-164): `bytes` sources return their
    /// data, `base64` sources decode, and `url` sources download through
    /// `fetch` carrying the transient [`Asset::headers`]; `ref` sources
    /// fail typed because the provider handle is not dereferenceable
    /// here. Divergence (G-246): the ref caches downloaded bytes on the
    /// instance (`#bytes`); the port re-downloads on every call — assets
    /// are plain data with no interior mutability.
    pub async fn bytes<F, Fut>(&self, fetch: F) -> Result<Vec<u8>, AiError>
    where
        F: FnOnce(String, BTreeMap<String, String>) -> Fut,
        Fut: Future<Output = Result<Vec<u8>, AiError>>,
    {
        match &self.source {
            Source::Bytes { data, .. } => Ok(data.0.clone()),
            Source::Base64 { data, .. } => {
                crate::protocols::shared::decode_base64(data).map_err(|cause| {
                    invalid_request_cause(
                        "Media asset contains invalid base64 data",
                        Some(cause),
                    )
                })
            }
            Source::Ref { provider, id, .. } => Err(invalid_request_cause(
                format!("Cannot materialize provider ref {}:{}", provider.0, id),
                None,
            )),
            Source::Url { url, .. } => {
                let headers = self.headers.clone().unwrap_or_default();
                fetch(url.clone(), headers).await
            }
        }
    }

    /// Pull `url` sources into owned bytes before the URL expires
    /// (media.ts:192-200); inline sources return themselves. The fresh
    /// asset keeps `info`/`provider_metadata` and sniffs an undeclared
    /// media type from magic bytes (the `Media.bytes` derive,
    /// media.ts:273-274); the transient headers do not carry over.
    pub async fn materialize<F, Fut>(&self, fetch: F) -> Result<Asset, AiError>
    where
        F: FnOnce(String, BTreeMap<String, String>) -> Fut,
        Fut: Future<Output = Result<Vec<u8>, AiError>>,
    {
        match &self.source {
            Source::Bytes { .. } | Source::Base64 { .. } => Ok(self.clone()),
            Source::Url { media_type, .. } | Source::Ref { media_type, .. } => {
                let data = self.bytes(fetch).await?;
                let media_type = media_type
                    .clone()
                    .or_else(|| crate::media::utils::detect_media_type(&data).map(str::to_string));
                let mut asset = bytes(data, media_type);
                asset.info = self.info.clone();
                asset.provider_metadata = self.provider_metadata.clone();
                Ok(asset)
            }
        }
    }
}

/// `Media.file` (media.ts:305-313): read a file and sniff its media type
/// from magic bytes, then the path extension (media-type.ts
/// `fileMediaType`), then `application/octet-stream`.
pub async fn file(path: &str) -> Result<Asset, AiError> {
    let data = tokio::fs::read(path).await.map_err(|cause| {
        invalid_request_cause(
            format!("Failed to read media file {path}"),
            Some(cause.to_string()),
        )
    })?;
    let media_type = crate::media::utils::file_media_type(&data, path);
    Ok(bytes(data, Some(media_type)))
}

/// `invalid(message, cause)` (media.ts:98-99): the typed constructor-error
/// shape shared by the runtime asset paths.
fn invalid_request_cause(message: impl Into<String>, cause: Option<String>) -> AiError {
    AiError::new(AiErrorReason::InvalidRequest {
        fields: ReasonFields {
            message: message.into(),
            body: None,
            http: None,
            cause,
        },
        parameter: None,
        classification: None,
    })
}

/// A provider-side partial result such as stripped audio or a moderated
/// sample; never a silent drop (port of `Media.Notice`, media.ts:87-92).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Notice {
    #[serde(rename = "type")]
    pub kind: NoticeType,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

/// `Media.Notice["type"]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NoticeType {
    #[serde(rename = "moderated")]
    Moderated,
    #[serde(rename = "filtered")]
    Filtered,
    #[serde(rename = "other")]
    Other,
}

/// `${number}:${number}` aspect ratio (media.ts:56-60); any string is
/// accepted at runtime, this validates the documented pattern.
#[must_use]
pub fn is_aspect_ratio(value: &str) -> bool {
    let parts: Vec<&str> = value.split(':').collect();
    if parts.len() != 2 {
        return false;
    }
    [parts[0], parts[1]]
        .iter()
        .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit() || c == '.'))
}

/// Constructors mirroring `Media.bytes/base64/url/ref`.
#[must_use]
pub fn bytes(data: Vec<u8>, media_type: Option<String>) -> Asset {
    let media_type = media_type.unwrap_or_else(|| "application/octet-stream".into());
    Asset {
        media_type: media_type.clone(),
        source: Source::Bytes { data: BytesPayload(data), media_type },
        info: None,
        expires_at: None,
        provider_metadata: None,
        headers: None,
    }
}

pub fn base64(data: impl Into<String>, media_type: impl Into<String>) -> Asset {
    let media_type = media_type.into();
    Asset {
        media_type: media_type.clone(),
        source: Source::Base64 { data: data.into(), media_type },
        info: None,
        expires_at: None,
        provider_metadata: None,
        headers: None,
    }
}

pub fn url(value: impl Into<String>, media_type: Option<String>, expires_at: Option<f64>) -> Asset {
    let expires = expires_at;
    Asset {
        media_type: media_type.clone().unwrap_or_else(|| "application/octet-stream".into()),
        expires_at: expires,
        source: Source::Url { url: value.into(), media_type, expires_at: expires },
        info: None,
        provider_metadata: None,
        headers: None,
    }
}

pub fn reference(
    provider: impl Into<String>,
    id: impl Into<String>,
    media_type: Option<String>,
) -> Asset {
    Asset {
        media_type: media_type.clone().unwrap_or_else(|| "application/octet-stream".into()),
        expires_at: None,
        source: Source::Ref { provider: ProviderId::new(provider), id: id.into(), media_type },
        info: None,
        provider_metadata: None,
        headers: None,
    }
}

/// Parse a `data:<mime>;base64,<data>` URL; `None` when not a base64 data URL.
#[must_use]
pub fn parse_data_url(value: &str) -> Option<Asset> {
    let rest = value.strip_prefix("data:")?;
    let (meta, data) = rest.split_once(',')?;

    if !meta.contains(";base64") {
        return None;
    }
    let mime = meta.split(';').next()?;
    if mime.is_empty() || data.is_empty() {
        return None;
    }
    Some(base64(data, mime))
}

/// Extra fields tolerated on asset JSON (matches TS's lenient encoded form).
#[must_use]
pub fn asset_from_json(value: &Value) -> Option<Asset> {
    serde_json::from_value(value.clone()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_follow_media_type_prefixes() {
        assert_eq!(kind_of("image/png"), Kind::Image);
        assert_eq!(kind_of("VIDEO/MP4"), Kind::Video);
        assert_eq!(kind_of("audio/wav"), Kind::Audio);
        assert_eq!(kind_of("application/pdf"), Kind::Document);
        assert_eq!(kind_of("text/html"), Kind::Document);
        assert_eq!(kind_of("application/json"), Kind::Other);
    }

    #[test]
    fn base64_assets_expose_inline_views() {
        let asset = base64("AQID", "image/png");
        let inline = asset.inline().expect("base64 is inline");
        assert_eq!(inline.mime, "image/png");
        assert_eq!(inline.data_url, "data:image/png;base64,AQID");
        assert_eq!(asset.kind(), Kind::Image);
        assert!(asset.url().is_none());
    }

    #[test]
    fn url_assets_have_no_inline_payload() {
        let asset = url("https://example.test/a.png", Some("image/png".into()), Some(123.0));
        assert!(asset.inline().is_none());
        assert_eq!(asset.url(), Some("https://example.test/a.png"));
        assert_eq!(asset.expires_at, Some(123.0));
        assert_eq!(asset.kind(), Kind::Image);
    }

    #[test]
    fn parses_base64_data_urls() {
        let asset = parse_data_url("data:image/jpeg;base64,QUJD").expect("valid data url");
        assert_eq!(asset.media_type, "image/jpeg");
        let inline = asset.inline().unwrap();
        assert_eq!(inline.base64, "QUJD");
        assert!(parse_data_url("https://example.test/x").is_none());
        assert!(parse_data_url("data:text/plain,hello").is_none());
    }

    #[test]
    fn assets_round_trip_through_json() {
        let asset = reference("openai", "file_123", Some("application/pdf".into()));
        let json = serde_json::to_value(&asset).unwrap();
        assert_eq!(json["source"]["type"], "ref");
        assert_eq!(json["source"]["provider"], "openai");
        let back: Asset = serde_json::from_value(json).unwrap();
        assert_eq!(back, asset);
        assert_eq!(back.kind(), Kind::Document);
    }

    // G-246: transient download credentials never reach the serde face —
    // a persisted asset cannot leak them (media.ts:208-213).
    #[test]
    fn transient_headers_never_serialize() {
        let mut headers = BTreeMap::new();
        headers.insert("x-goog-api-key".to_string(), "secret".to_string());
        let mut asset = url("https://v/1.mp4", Some("video/mp4".into()), Some(1.0));
        asset.headers = Some(headers);
        let json = serde_json::to_value(&asset).unwrap();
        assert!(json.get("headers").is_none(), "asset JSON carries no headers: {json}");
        let rendered = json.to_string();
        assert!(!rendered.contains("secret") && !rendered.contains("x-goog-api-key"));
        // A round-trip drops them: the deserialized asset cannot re-download.
        let back: Asset = serde_json::from_value(json).unwrap();
        assert_eq!(back.headers, None);
        assert_eq!(back.url(), Some("https://v/1.mp4"));
    }

    // G-246: `bytes()`/`materialize()` runtime semantics (media.ts:149-200).
    #[tokio::test]
    async fn bytes_decodes_inline_sources_and_fails_refs() {
        let owned = bytes(vec![1, 2, 3], Some("image/png".into()));
        assert_eq!(owned.bytes(|_url, _headers| async { Ok(Vec::new()) }).await.unwrap(), vec![1, 2, 3]);

        let encoded = base64("AQID", "image/png");
        assert_eq!(
            encoded.bytes(|_url, _headers| async { Ok(Vec::new()) }).await.unwrap(),
            vec![1, 2, 3]
        );

        let broken = base64("!!", "image/png");
        let error = broken.bytes(|_url, _headers| async { Ok(Vec::new()) }).await.unwrap_err();
        assert!(error.message().contains("invalid base64 data"), "{}", error.message());

        let handle = reference("openai", "file_123", None);
        let error = handle.bytes(|_url, _headers| async { Ok(Vec::new()) }).await.unwrap_err();
        assert!(error.message().contains("Cannot materialize provider ref openai:file_123"));
    }

    #[tokio::test]
    async fn bytes_downloads_url_sources_with_the_transient_headers() {
        let mut headers = BTreeMap::new();
        headers.insert("authorization".to_string(), "Bearer k".to_string());
        let mut asset = url("https://v/1.mp4", Some("video/mp4".into()), None);
        asset.headers = Some(headers);
        let seen = asset
            .bytes(|url, headers| async move {
                assert_eq!(url, "https://v/1.mp4");
                assert_eq!(headers.get("authorization").map(String::as_str), Some("Bearer k"));
                Ok(vec![9, 9])
            })
            .await
            .unwrap();
        assert_eq!(seen, vec![9, 9]);
    }

    #[tokio::test]
    async fn materialize_owns_url_sources_and_keeps_metadata() {
        let mut asset = url("https://s/1.png", None, Some(123.0));
        asset.info = Some(Info { width: Some(64.0), ..Info::default() });
        let mut metadata = crate::schema::options::ProviderMetadata::new();
        metadata.insert("x".to_string(), BTreeMap::new());
        asset.provider_metadata = Some(metadata);
        let mut headers = BTreeMap::new();
        headers.insert("authorization".to_string(), "Bearer k".to_string());
        asset.headers = Some(headers);
        let owned = asset
            .materialize(|_url, _headers| async { Ok(vec![0x89, 0x50, 0x4e, 0x47]) })
            .await
            .unwrap();
        // PNG magic sniffs the undeclared type (media.ts:273-274).
        assert_eq!(owned.media_type, "image/png");
        assert!(matches!(owned.source, Source::Bytes { .. }));
        assert_eq!(owned.info.as_ref().and_then(|info| info.width), Some(64.0));
        assert!(owned.provider_metadata.is_some());
        // the fresh asset carries no deadline and no credentials
        assert_eq!(owned.expires_at, None);
        assert_eq!(owned.headers, None);
        assert!(serde_json::to_value(&owned).unwrap().get("headers").is_none());

        // inline sources materialize to themselves
        let inline = base64("AQID", "image/png");
        let owned = inline
            .materialize(|_url, _headers| async { panic!("inline sources never fetch") })
            .await
            .unwrap();
        assert_eq!(owned, inline);
    }

    #[tokio::test]
    async fn file_reads_and_sniffs() {
        let path = "/tmp/opencode/gb10-lane6-file-asset-test.png";
        tokio::fs::write(path, [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 1, 2])
            .await
            .unwrap();
        let asset = file(path).await.unwrap();
        assert_eq!(asset.media_type, "image/png");
        // extension fallback when the magic bytes are unknown
        let plain = "/tmp/opencode/gb10-lane6-file-asset-test.md";
        tokio::fs::write(plain, "plain markdown").await.unwrap();
        let asset = file(plain).await.unwrap();
        assert_eq!(asset.media_type, "text/markdown");
        let missing = file("/tmp/opencode/gb10-lane6-definitely-missing.bin").await;
        assert!(missing.is_err());
    }
}
