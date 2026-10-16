//! `Media.Asset` type only (port of `src/media.ts` with media *protocols*
//! descoped per ai-map §10: ~6.4k LOC of image/video/speech/transcription
//! routes are out of scope for this lane).
//!
//! The asset type stays because messages/events carry media parts, and
//! protocols branch on `source.type` (`requireInlineMedia` / `mediaUrl` /
//! `mediaReference` — see [`crate::protocols::shared`]). Downloading `url`
//! sources and dereferencing `ref` sources belong to future media lanes; this
//! type only describes and inline-decodes.

use serde::{Deserialize, Serialize};
use serde_json::Value;

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
/// `Media.Asset`, minus the effectful byte cache/download paths).
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
    [parts[0], parts[1]].iter().all(|part| {
        !part.is_empty() && part.chars().all(|c| c.is_ascii_digit() || c == '.')
    })
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
}
