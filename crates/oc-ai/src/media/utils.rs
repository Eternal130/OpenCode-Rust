//! Shared media protocol utilities (ports of
//! `packages/ai/src/protocols/utils/{media-input,media-type,speech-stream,
//! speaker-turns}.ts`).

use crate::media::body::{merge_records, Body, Form, Query, QueryValue, normalize_numbers};
use crate::media::protocol::TextOutput;
use crate::schema::media::{self, Asset};
use crate::schema::{AiError, ProviderId, ProviderOptions};

// ===========================================================================
// media-input.ts
// ===========================================================================

/// Owned bytes for multipart uploads; decodes `base64` sources and rejects
/// remote sources (media-input.ts:9-16).
pub fn inline_bytes(route: &str, asset: &Asset) -> Result<Vec<u8>, AiError> {
    match &asset.source {
        media::Source::Bytes { data, .. } => Ok(data.0.clone()),
        _ => {
            let Some(inline) = asset.inline() else {
                return Err(crate::protocols::shared::inline_required(route, asset));
            };
            crate::protocols::shared::decode_base64(&inline.base64)
                .map_err(|_| crate::media::body::invalid_base64(route))
        }
    }
}

/// Build a query from a JSON record (media-input.ts:28-36): scalars pass
/// through, scalar arrays repeat, non-scalars fail typed.
pub fn query_from(route: &str, values: &serde_json::Map<String, serde_json::Value>) -> Result<Query, AiError> {
    let mut out = Query::default();
    for (key, value) in values {
        if value.is_null() {
            continue;
        }
        match value {
            serde_json::Value::String(text) => {
                out.0.insert(key.clone(), QueryValue::One(text.clone()));
            }
            serde_json::Value::Number(number) => {
                out.0.insert(key.clone(), QueryValue::One(number.to_string()));
            }
            serde_json::Value::Bool(flag) => {
                out.0.insert(key.clone(), QueryValue::One(flag.to_string()));
            }
            serde_json::Value::Array(items)
                if !items.is_empty()
                    && items.iter().all(|item| {
                        matches!(
                            item,
                            serde_json::Value::String(_) | serde_json::Value::Number(_) | serde_json::Value::Bool(_)
                        )
                    }) =>
            {
                out.0.insert(key.clone(), QueryValue::Many(items.iter().map(scalar_text).collect()));
            }
            _ => {
                return Err(crate::protocols::shared::invalid_request(format!(
                    "{route} cannot send \"{key}\" as a query parameter"
                )));
            }
        }
    }
    Ok(out)
}

/// `dimensions("WxH")` (media-input.ts:38-41).
#[must_use]
pub fn dimensions(size: &str) -> (Option<f64>, Option<f64>) {
    let mut split = size.split('x');
    let width = split.next().and_then(|value| value.parse::<f64>().ok());
    let height = split.next().and_then(|value| value.parse::<f64>().ok());
    (width, height)
}

/// Provider file handle when the ref belongs to this provider; refs from
/// other providers are never forwarded (media-input.ts:44-45).
#[must_use]
pub fn ref_id<'a>(asset: &'a Asset, provider: &ProviderId) -> Option<&'a str> {
    match &asset.source {
        media::Source::Ref { provider: source, id, .. } if source == provider => Some(id.as_str()),
        _ => None,
    }
}

/// Decode a provider's base64 output once into an owned `bytes` asset
/// (media-input.ts:48-58). Sniffs the type from magic bytes when it is not
/// declared (TS `Media.bytes` derives: declared, sniffed, octet-stream).
pub fn decoded_asset(
    invalid: impl FnOnce(String) -> AiError,
    label: &str,
    data: &str,
    media_type: Option<&str>,
    info: Option<media::Info>,
    provider_metadata: Option<crate::schema::ProviderMetadata>,
) -> Result<Asset, AiError> {
    let bytes = crate::protocols::shared::decode_base64(data)
        .map_err(|_| invalid(format!("{label} contains invalid base64 data")))?;
    let sniffed = media_type.map(str::to_string).or_else(|| detect_media_type(&bytes).map(str::to_string));
    let mut asset = media::bytes(bytes, sniffed);
    asset.info = info;
    asset.provider_metadata = provider_metadata;
    Ok(asset)
}

/// One image of an OpenAI-shaped `data` array, which carries either
/// `b64_json` or a `url` (media-input.ts:61-71).
pub fn image_output(
    invalid: impl FnOnce(String) -> AiError,
    label: &str,
    item: &serde_json::Value,
    media_type: Option<&str>,
    info: Option<media::Info>,
    provider_metadata: Option<crate::schema::ProviderMetadata>,
) -> Result<Asset, AiError> {
    if let Some(data) = item.get("b64_json").and_then(|value| value.as_str()) {
        if !data.is_empty() {
            return decoded_asset(invalid, label, data, media_type, info, provider_metadata);
        }
    }
    if let Some(url) = item.get("url").and_then(|value| value.as_str()) {
        if !url.is_empty() {
            let mut asset = media::url(url, media_type.map(str::to_string), None);
            asset.info = info;
            asset.provider_metadata = provider_metadata;
            return Ok(asset);
        }
    }
    Err(invalid(format!("{label} has neither image data nor a URL")))
}

/// Append multipart text fields (media-input.ts:78-93): strings as-is,
/// other values as JSON, or scalar arrays as one part per item with
/// `repeat_arrays`, named `key[]` or `key`. `overlay` keys in `reserved`
/// are dropped so `http.body` cannot replace route-owned fields.
pub fn append_fields(
    form: &mut Form,
    fields: Vec<(String, serde_json::Value)>,
    overlay: Option<&serde_json::Map<String, serde_json::Value>>,
    reserved: &[&str],
    repeat_arrays: Option<RepeatArrays>,
) {
    let filtered: Vec<(String, serde_json::Value)> = overlay
        .map(|overlay| {
            overlay
                .iter()
                .filter(|(key, _)| !reserved.contains(&key.as_str()))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        })
        .unwrap_or_default();
    let Some(merged) = crate::media::body::merge_records(&[
        (!fields.is_empty()).then(|| serde_json::Map::from_iter(fields)).as_ref(),
        (!filtered.is_empty()).then(|| filtered.into_iter().collect::<serde_json::Map<String, serde_json::Value>>()).as_ref(),
    ]) else {
        return;
    };
    for (key, value) in merged {
        let is_scalar = |value: &serde_json::Value| {
            matches!(value, serde_json::Value::String(_) | serde_json::Value::Number(_) | serde_json::Value::Bool(_))
        };
        if let (Some(mode), serde_json::Value::Array(items)) = (repeat_arrays, &value) {
            if !items.is_empty() && items.iter().all(is_scalar) {
                for item in items {
                    form.append_field(
                        if mode == RepeatArrays::Bracket { format!("{key}[]") } else { key.clone() },
                        scalar_text(item),
                    );
                }
                continue;
            }
        }
        // Strings append as-is; other values as JSON (media-input.ts:92)
        // with JS-number formatting.
        form.append_field(key, match &value {
            serde_json::Value::String(text) => text.clone(),
            other => serde_json::to_string(&normalize_numbers(other)).unwrap_or_default(),
        });
    }
}

/// `appendFields`'s `repeatArrays` naming (media-input.ts:84).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeatArrays {
    /// `key[]`
    Bracket,
    /// `key`
    Key,
}

fn scalar_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

// ===========================================================================
// media-type.ts
// ===========================================================================

/// Sniff a media type from leading magic bytes (media-type.ts
/// `detectMediaType`); anything else is `None` so callers fall back to a
/// provider-declared type.
#[must_use]
pub fn detect_media_type(bytes: &[u8]) -> Option<&'static str> {
    let starts_with = |prefix: &[u8]| bytes.len() >= prefix.len() && &bytes[..prefix.len()] == prefix;
    if starts_with(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]) {
        return Some("image/png");
    }
    if starts_with(&[0xff, 0xd8, 0xff]) {
        return Some("image/jpeg");
    }
    if starts_with(&[0x47, 0x49, 0x46, 0x38]) {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" {
        return match &bytes[8..12] {
            b"WEBP" => Some("image/webp"),
            b"WAVE" => Some("audio/wav"),
            _ => None,
        };
    }
    if starts_with(&[0x25, 0x50, 0x44, 0x46]) {
        return Some("application/pdf");
    }
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        return Some("video/mp4");
    }
    if starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        return Some("video/webm");
    }
    if starts_with(&[0x49, 0x44, 0x33]) {
        return Some("audio/mpeg");
    }
    // An 11-bit frame sync; layer bits `00` mark AAC ADTS, any other layer
    // is MPEG audio.
    if bytes.len() >= 2 && bytes[0] == 0xff && (bytes[1] & 0xe0) == 0xe0 {
        return if (bytes[1] & 0x06) == 0 { Some("audio/aac") } else { Some("audio/mpeg") };
    }
    if starts_with(&[0x4f, 0x67, 0x67, 0x53]) {
        return Some("audio/ogg");
    }
    if starts_with(&[0x66, 0x4c, 0x61, 0x43]) {
        return Some("audio/flac");
    }
    None
}

const EXTENSIONS: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("pdf", "application/pdf"),
    ("mp4", "video/mp4"),
    ("webm", "video/webm"),
    ("mp3", "audio/mpeg"),
    ("m4a", "audio/mp4"),
    ("wav", "audio/wav"),
    ("ogg", "audio/ogg"),
    ("flac", "audio/flac"),
    ("aac", "audio/aac"),
    ("txt", "text/plain"),
    ("md", "text/markdown"),
    ("csv", "text/csv"),
];

const EXTENSION_ALIASES: &[(&str, &str)] = &[
    ("audio/mp3", "mp3"),
    ("audio/m4a", "m4a"),
    ("audio/x-m4a", "m4a"),
    ("audio/webm", "webm"),
    ("audio/wave", "wav"),
    ("audio/x-wav", "wav"),
    ("audio/x-flac", "flac"),
];

/// The upload filename extension for a media type, for APIs that detect
/// the format from it (media-type.ts `mediaTypeExtension`).
#[must_use]
pub fn media_type_extension(media_type: &str) -> Option<&'static str> {
    let base = media_type.split(';').next().unwrap_or_default().trim().to_ascii_lowercase();
    if let Some((_, extension)) = EXTENSION_ALIASES.iter().find(|(known, _)| *known == base) {
        return Some(extension);
    }
    EXTENSIONS.iter().find(|(_, known)| *known == base).map(|(extension, _)| *extension)
}

// ===========================================================================
// gemini-generate-content.ts
// ===========================================================================

/// The shared Gemini generate-content helpers (gemini-generate-content.ts).
pub mod gemini {
    use crate::media::protocol::{Framing, Identity};
    use crate::schema::media::{self, Info, Notice, NoticeType};
    use crate::schema::{AiError, AiErrorReason, MediaUsage};

    /// Usage metadata seen on chunks (gemini-generate-content.ts:15-21).
    #[derive(Debug, Clone, Default, PartialEq)]
    pub struct UsageMetadata {
        pub prompt_token_count: Option<f64>,
        pub candidates_token_count: Option<f64>,
        pub total_token_count: Option<f64>,
    }

    impl UsageMetadata {
        fn from_value(value: &serde_json::Value) -> Self {
            Self {
                prompt_token_count: value.get("promptTokenCount").and_then(|v| v.as_f64()),
                candidates_token_count: value.get("candidatesTokenCount").and_then(|v| v.as_f64()),
                total_token_count: value.get("totalTokenCount").and_then(|v| v.as_f64()),
            }
        }
    }

    /// The tracked chunk metadata (gemini-generate-content.ts:70-76).
    #[derive(Debug, Clone, Default, PartialEq)]
    pub struct Metadata {
        pub usage: Option<UsageMetadata>,
        pub finish_reason: Option<String>,
        pub model_version: Option<String>,
        pub response_id: Option<String>,
    }

    /// Decode one generate-content chunk (any parts shape kept raw).
    pub fn decode_chunk(frame: &str) -> Result<serde_json::Value, AiError> {
        serde_json::from_str(frame).map_err(|cause| {
            AiError::new(AiErrorReason::InvalidProviderOutput {
                fields: crate::schema::ReasonFields {
                    message: format!("invalid generateContent chunk: {cause}"),
                    body: Some(frame.to_string()),
                    http: None,
                    cause: None,
                },
                classification: None,
                route: None,
            })
        })
    }

    /// `track` (gemini-generate-content.ts:78-84): later chunks win.
    pub fn track(state: &mut Metadata, chunk: &serde_json::Value) {
        if let Some(usage) = chunk.get("usageMetadata").filter(|value| !value.is_null()) {
            state.usage = Some(UsageMetadata::from_value(usage));
        }
        if let Some(reason) = chunk.pointer("/candidates/0/finishReason").and_then(|v| v.as_str()) {
            state.finish_reason = Some(reason.to_string());
        }
        if let Some(version) = chunk.get("modelVersion").and_then(|v| v.as_str()) {
            state.model_version = Some(version.to_string());
        }
        if let Some(id) = chunk.get("responseId").and_then(|v| v.as_str()) {
            state.response_id = Some(id.to_string());
        }
    }

    /// `blocked` (gemini-generate-content.ts:86-95): a prompt-level block.
    pub fn blocked(name: &str, chunk: &serde_json::Value, frame: &str) -> Option<AiError> {
        let reason = chunk.pointer("/promptFeedback/blockReason").and_then(|v| v.as_str())?;
        let message_detail = chunk
            .pointer("/promptFeedback/blockReasonMessage")
            .and_then(|v| v.as_str())
            .map(|message| format!(": {message}"))
            .unwrap_or_default();
        Some(AiError::new(AiErrorReason::ContentPolicy {
            fields: crate::schema::ReasonFields {
                message: format!("{name} blocked the request ({reason}){message_detail}"),
                body: Some(frame.to_string()),
                http: None,
                cause: None,
            },
        }))
    }

    /// Finish reasons for which Gemini stops output on safety or policy
    /// grounds (gemini-generate-content.ts:97-108).
    fn content_filtered(finish_reason: Option<&str>) -> bool {
        const REASONS: &[&str] = &[
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
        finish_reason.is_some_and(|reason| REASONS.contains(&reason))
    }

    /// `withheld` (gemini-generate-content.ts:110-116): no output at all
    /// after a policy stop.
    pub fn withheld(name: &str, chunk: &serde_json::Value, frame: &str) -> Option<AiError> {
        let reason = chunk.pointer("/candidates/0/finishReason").and_then(|v| v.as_str());
        if !content_filtered(reason) {
            return None;
        }
        Some(AiError::new(AiErrorReason::ContentPolicy {
            fields: crate::schema::ReasonFields {
                message: format!("{name} withheld its output ({})", reason.unwrap_or_default()),
                body: Some(frame.to_string()),
                http: None,
                cause: None,
            },
        }))
    }

    /// `notices` (gemini-generate-content.ts:100-110): any finish reason
    /// other than `STOP` is surfaced rather than dropped.
    pub fn notices(name: &str, state: &Metadata) -> Option<Vec<Notice>> {
        let reason = state.finish_reason.as_deref()?;
        if reason == "STOP" {
            return None;
        }
        let mut google = std::collections::BTreeMap::new();
        google.insert("finishReason".to_string(), serde_json::json!(reason));
        let mut metadata = crate::schema::ProviderMetadata::new();
        metadata.insert("google".to_string(), google);
        Some(vec![Notice {
            kind: if content_filtered(Some(reason)) { NoticeType::Filtered } else { NoticeType::Other },
            message: format!("{name} finished with {reason}"),
            provider_metadata: Some(metadata),
        }])
    }

    /// `usage` (gemini-generate-content.ts:112-121).
    pub fn usage(usage: &Option<UsageMetadata>) -> Option<MediaUsage> {
        let usage = usage.as_ref()?;
        Some(MediaUsage::Tokens {
            input: usage.prompt_token_count,
            output: usage.candidates_token_count,
            total: crate::protocols::shared::total_tokens(
                usage.prompt_token_count,
                usage.candidates_token_count,
                usage.total_token_count,
            ),
            details: None,
        })
    }

    /// `providerMetadata` (gemini-generate-content.ts:123-125).
    pub fn provider_metadata(state: &Metadata) -> crate::schema::ProviderMetadata {
        let mut google = std::collections::BTreeMap::new();
        google.insert("finishReason".to_string(), serde_json::json!(state.finish_reason));
        google.insert("modelVersion".to_string(), serde_json::json!(state.model_version));
        google.insert("responseId".to_string(), serde_json::json!(state.response_id));
        let mut metadata = crate::schema::ProviderMetadata::new();
        metadata.insert("google".to_string(), google);
        metadata
    }

    /// `path` (gemini-generate-content.ts:127-128).
    #[must_use]
    pub fn path(model: &str, mode: crate::media::model::Mode) -> String {
        match mode {
            crate::media::model::Mode::Stream => format!("/models/{model}:streamGenerateContent?alt=sse"),
            crate::media::model::Mode::Generate => format!("/models/{model}:generateContent"),
        }
    }

    /// `frames` (gemini-generate-content.ts:130-132).
    #[must_use]
    pub fn framing(mode: crate::media::model::Mode) -> Framing {
        match mode {
            crate::media::model::Mode::Stream => Framing::Sse,
            crate::media::model::Mode::Generate => Framing::Document,
        }
    }

    /// `mediaPart` (gemini-generate-content.ts:134-149): Gemini does not
    /// fetch public URLs; inline payloads and Gemini Files references are
    /// the accepted inputs.
    pub fn media_part(route: &str, asset: &media::Asset) -> Result<serde_json::Value, AiError> {
        if let Some(file_uri) = super::ref_id(asset, &crate::schema::ProviderId::new("google")) {
            return Ok(serde_json::json!({ "fileData": { "mimeType": asset.media_type, "fileUri": file_uri } }));
        }
        let inline = super::require_inline(route, asset)?;
        Ok(serde_json::json!({ "inlineData": { "mimeType": inline.mime, "data": inline.base64 } }))
    }

    /// Keep the `Info` import referenced for the module's documented type
    /// surface.
    #[allow(dead_code)]
    fn _info_bound(_: Option<Info>) {}

    /// Keep the `Identity` import referenced (error constructors take it
    /// in callers).
    #[allow(dead_code)]
    fn _identity_bound(_: &Identity) {}
}


// ===========================================================================
// speaker-turns.ts
// ===========================================================================

/// Split an ordered token list into runs of consecutive tokens with the
/// same speaker (speaker-turns.ts:1-9).
pub mod speaker_turns {
    /// Group by consecutive equal speaker keys; the key is compared by
    /// rendered `serde_json` value so `0` and `"0"` stay distinct.
    pub fn group<T, K: PartialEq>(items: &[T], speaker: impl Fn(&T) -> K) -> Vec<&[T]> {
        let mut turns: Vec<(K, usize, usize)> = Vec::new();
        for (index, item) in items.iter().enumerate() {
            let key = speaker(item);
            match turns.last_mut() {
                Some((last, start, end)) if *last == key => *end = index,
                _ => turns.push((key, index, index)),
            }
        }
        turns.into_iter().map(|(_, start, end)| &items[start..=end]).collect()
    }
}

// ===========================================================================
// speech-stream.ts
// ===========================================================================

/// The speech-stream audio accumulator + finish helper
/// (speech-stream.ts:14-111).
pub mod speech_stream {
    use crate::media::model::{SpeechEvent, SpeechTimestamp};
    use crate::media::protocol::{Identity, TextOutput};
    use crate::schema::media::{self, Info, Notice};
    use crate::schema::{AiError, MediaUsage};

    /// `SpeechStream.Audio` — appended in place; the route creates fresh
    /// state per response through `initial`.
    #[derive(Debug, Clone, Default, PartialEq)]
    pub struct Audio {
        pub chunks: Vec<Vec<u8>>,
    }

    /// Empty chunks (keep-alive records) emit nothing (speech-stream.ts:22-26).
    pub fn delta(state: &mut Audio, chunk: Vec<u8>) -> Vec<SpeechEvent> {
        if chunk.is_empty() {
            return Vec::new();
        }
        state.chunks.push(chunk.clone());
        vec![SpeechEvent::AudioDelta { chunk }]
    }

    /// `SpeechStream.timestamps` (speech-stream.ts:33-39): missing offsets
    /// default to 0.
    pub fn timestamps(texts: &[String], starts: &[f64], ends: &[f64]) -> Vec<SpeechEvent> {
        if texts.is_empty() {
            return Vec::new();
        }
        vec![SpeechEvent::Timestamps {
            items: texts
                .iter()
                .enumerate()
                .map(|(index, text)| SpeechTimestamp {
                    text: text.clone(),
                    start_seconds: starts.get(index).copied().unwrap_or(0.0),
                    end_seconds: ends.get(index).copied().unwrap_or(0.0),
                })
                .collect(),
        }]
    }

    const CONTAINER_MEDIA_TYPES: &[(&str, &str)] = &[
        ("mp3", "audio/mpeg"),
        ("wav", "audio/wav"),
        ("opus", "audio/ogg"),
        ("aac", "audio/aac"),
        ("flac", "audio/flac"),
    ];

    /// The output description for a container format (speech-stream.ts:56-59).
    #[derive(Debug, Clone, PartialEq)]
    pub struct Output {
        pub media_type: Option<String>,
        pub info: Option<Info>,
    }

    /// `SpeechStream.container` (speech-stream.ts:56-59).
    #[must_use]
    pub fn container(format: &str, sample_rate: Option<f64>) -> Output {
        Output {
            media_type: CONTAINER_MEDIA_TYPES
                .iter()
                .find(|(known, _)| *known == format)
                .map(|(_, media_type)| (*media_type).to_string()),
            info: Some(Info { format: Some(format.to_string()), sample_rate, ..Info::default() }),
        }
    }

    /// The PCM encodings (speech-stream.ts:61-68).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum PcmEncoding {
        #[allow(non_camel_case_types)]
        pcm_s16le,
        #[allow(non_camel_case_types)]
        pcm_f32le,
        #[allow(non_camel_case_types)]
        pcm_mulaw,
        #[allow(non_camel_case_types)]
        pcm_alaw,
    }

    impl PcmEncoding {
        /// The media type for the encoding.
        #[must_use]
        pub fn media_type(self) -> &'static str {
            match self {
                Self::pcm_s16le | Self::pcm_f32le => "audio/pcm",
                Self::pcm_mulaw => "audio/mulaw",
                Self::pcm_alaw => "audio/alaw",
            }
        }

        /// Parse the wire name.
        #[must_use]
        pub fn parse(value: &str) -> Option<Self> {
            match value {
                "pcm_s16le" => Some(Self::pcm_s16le),
                "pcm_f32le" => Some(Self::pcm_f32le),
                "pcm_mulaw" => Some(Self::pcm_mulaw),
                "pcm_alaw" => Some(Self::pcm_alaw),
                _ => None,
            }
        }
    }

    /// `SpeechStream.pcm` (speech-stream.ts:70-73).
    #[must_use]
    pub fn pcm(encoding: PcmEncoding, sample_rate: Option<f64>, media_type: Option<&str>) -> Output {
        Output {
            media_type: Some(media_type.unwrap_or(encoding.media_type()).to_string()),
            info: Some(Info {
                format: Some("pcm".to_string()),
                encoding: Some(match encoding {
                    PcmEncoding::pcm_s16le => "pcm_s16le",
                    PcmEncoding::pcm_f32le => "pcm_f32le",
                    PcmEncoding::pcm_mulaw => "pcm_mulaw",
                    PcmEncoding::pcm_alaw => "pcm_alaw",
                }
                .to_string()),
                sample_rate,
                channels: Some(1.0),
                ..Info::default()
            }),
        }
    }

    /// `SpeechStream.sampleRate` — a `rate=<n>` parameter inside a content
    /// type (speech-stream.ts:75-78).
    #[must_use]
    pub fn sample_rate(media_type: Option<&str>) -> Option<f64> {
        let media_type = media_type?;
        let index = media_type.to_ascii_lowercase().find("rate=")?;
        media_type[index + 5..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .ok()
    }

    /// `SpeechStream.finish` (speech-stream.ts:81-105): a stream that
    /// produced no audio at all fails; otherwise the concatenated bytes
    /// become the finish event's asset.
    pub fn finish(
        identity: &Identity,
        state: &Audio,
        output: Output,
        usage: Option<MediaUsage>,
        notices: Option<Vec<Notice>>,
        provider_metadata: Option<crate::schema::ProviderMetadata>,
        detail: Option<&str>,
    ) -> Result<Vec<SpeechEvent>, AiError> {
        if state.chunks.is_empty() {
            let message = match detail {
                Some(detail) => format!("The provider returned no audio ({detail})"),
                None => "The provider returned no audio".to_string(),
            };
            return Err(identity.frame_error(message, String::new()));
        }
        let mut bytes = Vec::new();
        for chunk in &state.chunks {
            bytes.extend_from_slice(chunk);
        }
        let mut asset = media::bytes(bytes, output.media_type.clone());
        asset.info = output.info;
        Ok(vec![SpeechEvent::Finish { audio: asset, usage, notices, provider_metadata }])
    }

    /// `SpeechStream.headerUsage` (speech-stream.ts:107-111): a header
    /// count parsed as a number, `None` when it is not finite.
    #[must_use]
    pub fn header_usage(kind: &str, value: Option<&str>) -> Option<MediaUsage> {
        let amount = value?.parse::<f64>().ok()?;
        if !amount.is_finite() {
            return None;
        }
        match kind {
            "credits" => Some(MediaUsage::Credits { credits: amount }),
            "characters" => Some(MediaUsage::Characters { characters: amount }),
            _ => None,
        }
    }

    /// `SpeechStream.voiceID` (speech-stream.ts:46).
    #[must_use]
    pub fn voice_id(voice: &Option<crate::media::model::SpeechVoice>) -> Option<&str> {
        voice.as_ref().map(crate::media::model::SpeechVoice::id)
    }

    /// The unused-import guard keeps `TextOutput` referenced for parity
    /// docs (decode contexts carry it).
    #[allow(dead_code)]
    fn _text_output_bound(_: &TextOutput) {}
}

// ===========================================================================
// shared.rs additions used only by media protocols
// ===========================================================================

/// `ProviderShared.mediaReference` (shared.ts:201-222): the one string a
/// provider can address an asset by — inline payloads as a data URL,
/// `url` sources as their URL, and this provider's own `ref` as its id.
/// Other providers' refs are never forwarded and fail typed.
pub fn media_reference(
    asset: &Asset,
    provider: Option<&ProviderId>,
    label: &str,
) -> Result<(MediaRefKind, String), AiError> {
    if let Some(inline) = asset.inline() {
        return Ok((MediaRefKind::DataUrl, inline.data_url));
    }
    if let Some(url) = asset.url() {
        return Ok((MediaRefKind::Url, url.to_string()));
    }
    if let (Some(provider), media::Source::Ref { provider: source, id, .. }) = (provider, &asset.source) {
        if source == provider {
            return Ok((MediaRefKind::Ref, id.clone()));
        }
    }
    let accepted = match provider {
        None => String::new(),
        Some(provider) => format!(", and {} references", provider.0),
    };
    let got = match &asset.source {
        media::Source::Ref { provider, id, .. } => format!("; got {}:{}", provider.0, id),
        _ => String::new(),
    };
    Err(crate::protocols::shared::invalid_request(format!(
        "{label} accepts inline bytes, data URLs, http(s) URLs{accepted}{got}"
    )))
}

/// `MediaReference["type"]` (shared.ts:201).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaRefKind {
    DataUrl,
    Url,
    Ref,
}

/// `ProviderShared.requireInlineMedia` (shared.ts:188-191).
pub fn require_inline(route: &str, asset: &Asset) -> Result<media::Inline, AiError> {
    asset
        .inline()
        .ok_or_else(|| crate::protocols::shared::inline_required(route, asset))
}

/// `ProviderShared.mediaUrl` (shared.ts:199).
#[must_use]
pub fn media_url(asset: &Asset) -> Option<&str> {
    asset.url()
}

/// Native options passthrough: remove keys the protocol lowers itself.
/// TS destructures them; here the caller names them.
#[must_use]
pub fn strip_options(options: &Option<ProviderOptions>, keys: &[&str]) -> Option<ProviderOptions> {
    let options = options.as_ref()?;
    let mut stripped = options.clone();
    for key in keys {
        stripped.remove(*key);
    }
    Some(stripped)
}

/// The `TextOutput` re-export used across protocol modules.
pub use crate::media::protocol::TextOutput as Output;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn inline_bytes_decodes_base64_and_rejects_urls() {
        let asset = media::base64("AQID", "image/png");
        assert_eq!(inline_bytes("r", &asset).unwrap(), vec![1, 2, 3]);
        let remote = media::url("https://x/a.png", Some("image/png".into()), None);
        let error = inline_bytes("r", &remote).unwrap_err();
        assert!(error.reason.tag() == "InvalidRequest");
    }

    #[test]
    fn query_from_rejects_objects() {
        let values: serde_json::Map<String, serde_json::Value> =
            [("a", json!(1)), ("b", json!(["x", "y"]))].into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        let query = query_from("r", &values).unwrap();
        assert_eq!(query.0["a"], QueryValue::One("1".to_string()));
        assert_eq!(query.0["b"], QueryValue::Many(vec!["x".to_string(), "y".to_string()]));
        let bad: serde_json::Map<String, serde_json::Value> =
            [("obj", json!({"x": 1}))].into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        let error = query_from("r", &bad).unwrap_err();
        assert!(error.message().contains("cannot send \"obj\" as a query parameter"));
    }

    #[test]
    fn dimensions_splits_wxh() {
        assert_eq!(dimensions("1024x768"), (Some(1024.0), Some(768.0)));
    }

    #[test]
    fn media_type_extension_maps_uploads() {
        assert_eq!(media_type_extension("audio/mpeg"), Some("mp3"));
        assert_eq!(media_type_extension("audio/mp3"), Some("mp3"));
        assert_eq!(media_type_extension("audio/x-m4a;rate=44100"), Some("m4a"));
        assert_eq!(media_type_extension("application/weird"), None);
    }

    #[test]
    fn detect_media_type_sniffs_magic_bytes() {
        assert_eq!(detect_media_type(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), Some("image/png"));
        assert_eq!(detect_media_type(&[0xff, 0xd8, 0xff, 0xe0]), Some("image/jpeg"));
        assert_eq!(detect_media_type(&[0x1a, 0x45, 0xdf, 0xa3, 0x00]), Some("video/webm"));
        assert_eq!(detect_media_type(&[0x00, 0x01]), None);
    }

    #[test]
    fn speaker_turn_groups_consecutive_runs() {
        let items = vec![(1, "a"), (1, "b"), (2, "c"), (1, "d")];
        let turns = speaker_turns::group(&items, |item| item.0);
        assert_eq!(turns.len(), 3);
        assert_eq!(turns[0], &[(1, "a"), (1, "b")][..]);
        assert_eq!(turns[1], &[(2, "c")][..]);
        assert_eq!(turns[2], &[(1, "d")][..]);
    }

    #[test]
    fn media_reference_prefers_inline_then_url_then_own_ref() {
        let inline = media::base64("AQID", "image/png");
        let (kind, value) = media_reference(&inline, None, "route").unwrap();
        assert_eq!(kind, MediaRefKind::DataUrl);
        assert_eq!(value, "data:image/png;base64,AQID");

        let url = media::url("https://x/a.png", None, None);
        let (kind, value) = media_reference(&url, None, "route").unwrap();
        assert_eq!((kind, value.as_str()), (MediaRefKind::Url, "https://x/a.png"));

        let own = media::reference("xai", "file_1", Some("image/png".into()));
        let (kind, value) = media_reference(&own, Some(&ProviderId::new("xai")), "route").unwrap();
        assert_eq!((kind, value.as_str()), (MediaRefKind::Ref, "file_1"));

        let foreign = media::reference("openai", "file_2", None);
        let error = media_reference(&foreign, Some(&ProviderId::new("xai")), "route").unwrap_err();
        assert!(error.message().contains("got openai:file_2"));
    }

    #[test]
    fn speech_stream_finish_fails_without_audio() {
        let identity = crate::media::protocol::Identity::new("openai-speech", "OpenAI Speech", "openai");
        let state = speech_stream::Audio::default();
        let error = speech_stream::finish(&identity, &state, speech_stream::container("mp3", None), None, None, None, None)
            .unwrap_err();
        assert_eq!(error.message(), "The provider returned no audio");
        let mut state = speech_stream::Audio::default();
        speech_stream::delta(&mut state, vec![1, 2]);
        speech_stream::delta(&mut state, vec![3]);
        let events =
            speech_stream::finish(&identity, &state, speech_stream::container("mp3", None), None, None, None, None)
                .unwrap();
        assert!(matches!(&events[0], crate::media::model::SpeechEvent::Finish { audio, .. } if matches!(audio.source, media::Source::Bytes { .. })));
    }

    #[test]
    fn header_usage_parses_counts() {
        assert_eq!(
            speech_stream::header_usage("characters", Some("123")),
            Some(crate::schema::MediaUsage::Characters { characters: 123.0 })
        );
        assert_eq!(speech_stream::header_usage("credits", Some("3")), Some(crate::schema::MediaUsage::Credits { credits: 3.0 }));
        assert_eq!(speech_stream::header_usage("credits", Some("nan")), None);
        assert_eq!(speech_stream::header_usage("credits", None), None);
    }
}
