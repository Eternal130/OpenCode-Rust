//! Media model types (ports of `packages/ai/src/image.ts`, `speech.ts`,
//! `video.ts`, `transcription.ts`, and `generation.ts`).
//!
//! One module instead of five TS files because the modality types are
//! structurally parallel and small; section banners cite the source file.
//! The request types carry the model **id** (the TS `ImageModel` object is
//! a route handle — deployment lives with a future media-route lane; the
//! protocol layer only needs `model.id` to lower a body).

use serde::{Deserialize, Serialize};

use super::body::Body;
use crate::schema::media::{Asset, Notice};
use crate::schema::{AiError, HttpOptions, MediaUsage, ProviderMetadata, ProviderOptions};

// ---------------------------------------------------------------------------
// generation.ts — Status / Snapshot / Mode
// ---------------------------------------------------------------------------

/// Generation lifecycle (`generation.ts:4`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    #[serde(rename = "queued")]
    Queued,
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "completed")]
    Completed,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "cancelled")]
    Cancelled,
    #[serde(rename = "expired")]
    Expired,
}

/// Whether the status is one the generation will not leave (generation.ts:54).
#[must_use]
pub fn is_terminal(status: Status) -> bool {
    matches!(
        status,
        Status::Completed | Status::Failed | Status::Cancelled | Status::Expired
    )
}

/// Provider-neutral view of one generation observation (generation.ts:8-14).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub id: String,
    pub status: Status,
    /// Normalized 0..1 when the provider reports progress.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<f64>,
}

/// The call mode a media route runs in (media-protocol.ts:155).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    #[serde(rename = "generate")]
    Generate,
    #[serde(rename = "stream")]
    Stream,
}

/// `generation-queued` (generation.ts:38-42).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueuedEvent {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<f64>,
}

/// `generation-progress` (generation.ts:44-48).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<f64>,
}

// ---------------------------------------------------------------------------
// image.ts
// ---------------------------------------------------------------------------

/// `Image.Request` (image.ts:65-81). `model` is the model id.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageRequest {
    pub model: String,
    pub prompt: String,
    /// Edit sources or style/subject references, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<Asset>,
    /// Inpainting mask; routes that cannot honor it fail `UnsupportedOperation`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<Asset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub n: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http: Option<HttpOptions>,
}

/// `Image.Response` (image.ts:107-116).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageResponse {
    pub images: Vec<Asset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<MediaUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notices: Option<Vec<Notice>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl ImageResponse {
    /// `Response.image` — the first generated image.
    #[must_use]
    pub fn image(&self) -> Option<&Asset> {
        self.images.first()
    }
}

/// `Image.Event` tagged union (image.ts:119-154).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum ImageEvent {
    #[serde(rename = "generation-queued")]
    GenerationQueued(QueuedEvent),
    #[serde(rename = "generation-progress")]
    GenerationProgress(ProgressEvent),
    /// The `index`-th lower-fidelity preview; the `image` event supersedes it.
    #[serde(rename = "image-partial")]
    ImagePartial {
        index: f64,
        image: Asset,
    },
    #[serde(rename = "image")]
    Image {
        index: f64,
        image: Asset,
    },
    #[serde(rename = "finish")]
    Finish {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<MediaUsage>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        notices: Option<Vec<Notice>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
}

// ---------------------------------------------------------------------------
// speech.ts
// ---------------------------------------------------------------------------

/// `Speech.Voice` — a name or `{ id }` for OpenAI custom voices.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SpeechVoice {
    Name(String),
    Custom { id: String },
}

impl SpeechVoice {
    /// `SpeechStream.voiceID` — the id-or-name string protocols send.
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Name(name) => name,
            Self::Custom { id } => id,
        }
    }
}

/// `Speech.Timestamp` — provider-native granularity (characters on
/// ElevenLabs, words on Cartesia).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechTimestamp {
    pub text: String,
    pub start_seconds: f64,
    pub end_seconds: f64,
}

/// `Speech.Request` (speech.ts:70-83).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechRequest {
    pub model: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<SpeechVoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// Opt-in booleans are rejected with `=== true` semantics inside
    /// `body.from`, not through `unsupported` (route/media.ts:419-423).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamps: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http: Option<HttpOptions>,
}

/// `Speech.Response` (speech.ts:109-116).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechResponse {
    /// The complete audio. Headerless PCM carries `info.encoding`,
    /// `info.sampleRate`, and `info.channels`.
    pub audio: Asset,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamps: Option<Vec<SpeechTimestamp>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<MediaUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notices: Option<Vec<Notice>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

/// `Speech.Event` tagged union (speech.ts:118-153).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum SpeechEvent {
    #[serde(rename = "generation-queued")]
    GenerationQueued(QueuedEvent),
    #[serde(rename = "generation-progress")]
    GenerationProgress(ProgressEvent),
    #[serde(rename = "audio-delta")]
    AudioDelta {
        /// Raw bytes (`Schema.Uint8Array` on the TS wire → JSON byte array).
        chunk: Vec<u8>,
    },
    #[serde(rename = "timestamps")]
    Timestamps { items: Vec<SpeechTimestamp> },
    /// `audio` is every `audio-delta` concatenated; the route holds the
    /// whole clip in memory until `finish`.
    #[serde(rename = "finish")]
    Finish {
        audio: Asset,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<MediaUsage>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        notices: Option<Vec<Notice>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
}

// ---------------------------------------------------------------------------
// video.ts
// ---------------------------------------------------------------------------

/// `Video.Frames` — pinned frames (video.ts:60-64).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoFrames {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first: Option<Asset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<Asset>,
}

/// `Video.Request` (video.ts:66-86).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoRequest {
    pub model: String,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frames: Option<VideoFrames>,
    /// Style or subject references that guide the output without pinning a frame.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<Asset>,
    /// Source video for edit or extension routes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video: Option<Asset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,
    /// Whether to generate an audio track; routes whose audio is always on
    /// fail typed on `false`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub n: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub negative_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http: Option<HttpOptions>,
}

/// `Video.Response` (video.ts:112-121).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoResponse {
    pub videos: Vec<Asset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<MediaUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notices: Option<Vec<Notice>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

impl VideoResponse {
    /// `Response.video` — the first generated video.
    #[must_use]
    pub fn video(&self) -> Option<&Asset> {
        self.videos.first()
    }
}

/// `Video.Event` tagged union (video.ts:123-147).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum VideoEvent {
    #[serde(rename = "generation-queued")]
    GenerationQueued(QueuedEvent),
    #[serde(rename = "generation-progress")]
    GenerationProgress(ProgressEvent),
    #[serde(rename = "video")]
    Video { index: f64, video: Asset },
    #[serde(rename = "finish")]
    Finish {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<MediaUsage>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        notices: Option<Vec<Notice>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },
}

// ---------------------------------------------------------------------------
// transcription.ts
// ---------------------------------------------------------------------------

/// `Transcription.Timestamps` (transcription.ts:65-66).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TranscriptionTimestamps {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "segment")]
    Segment,
    #[serde(rename = "word")]
    Word,
}

/// `Transcription.Request` (transcription.ts:68-81).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionRequest {
    pub model: String,
    pub audio: Asset,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// Routes that cannot produce the requested granularity fail typed;
    /// routes may return more than asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamps: Option<TranscriptionTimestamps>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diarize: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speakers: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http: Option<HttpOptions>,
}

/// `Transcription.Segment` — speaker labels are provider-native
/// (`A`, `0`, `spk:0`, `speaker_0`, or a known speaker name).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionSegment {
    pub text: String,
    pub start_seconds: f64,
    pub end_seconds: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
}

/// `Transcription.Word` (transcription.ts:115-122).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionWord {
    pub text: String,
    pub start_seconds: f64,
    pub end_seconds: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
}

/// `Transcription.Response` (transcription.ts:124-138).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionResponse {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub segments: Option<Vec<TranscriptionSegment>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub words: Option<Vec<TranscriptionWord>>,
    /// Provider-native language as detected or echoed (`en`, `english`,
    /// `en_us`), lowercased but not normalized.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<MediaUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notices: Option<Vec<Notice>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,
}

/// `Transcription.Event` tagged union (transcription.ts:140-169).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum TranscriptionEvent {
    #[serde(rename = "generation-queued")]
    GenerationQueued(QueuedEvent),
    #[serde(rename = "generation-progress")]
    GenerationProgress(ProgressEvent),
    #[serde(rename = "text-delta")]
    TextDelta { delta: String },
    #[serde(rename = "segment")]
    Segment { segment: TranscriptionSegment },
    #[serde(rename = "finish")]
    Finish(TranscriptionResponse),
}

// ---------------------------------------------------------------------------
// The erased protocol inputs/outputs
// ---------------------------------------------------------------------------

/// One media request of any modality (the protocol-layer erasure of the
/// four request types; TS keeps them distinct per protocol family).
#[derive(Debug, Clone)]
pub enum MediaRequest {
    Image(ImageRequest),
    Speech(SpeechRequest),
    Video(VideoRequest),
    Transcription(TranscriptionRequest),
}

impl MediaRequest {
    /// The model id every lowering needs.
    #[must_use]
    pub fn model_id(&self) -> &str {
        match self {
            Self::Image(request) => &request.model,
            Self::Speech(request) => &request.model,
            Self::Video(request) => &request.model,
            Self::Transcription(request) => &request.model,
        }
    }

    /// The caller's `http` overlay (body/headers/query).
    #[must_use]
    pub fn http(&self) -> Option<&HttpOptions> {
        match self {
            Self::Image(request) => request.http.as_ref(),
            Self::Speech(request) => request.http.as_ref(),
            Self::Video(request) => request.http.as_ref(),
            Self::Transcription(request) => request.http.as_ref(),
        }
    }
}

/// `MediaProtocol.Addressed<Request>` — a request plus its call mode.
#[derive(Debug, Clone)]
pub struct Addressed<T> {
    pub request: T,
    pub mode: Mode,
}

impl<T> Addressed<T> {
    #[must_use]
    pub fn new(request: T, mode: Mode) -> Self {
        Self { request, mode }
    }
}

/// `MediaProtocol.DecodeContext<Request>` (media-protocol.ts:59-62).
pub struct DecodeContext<'a, T> {
    pub request: &'a T,
    pub body: &'a Body,
}

/// `MediaProtocol.ResponseContext<Request>` (media-protocol.ts:159-161).
pub struct ResponseContext<'a, T> {
    pub request: &'a Addressed<T>,
    pub body: &'a Body,
    pub http: &'a crate::schema::HttpContext,
}

/// Any modality's response.
#[derive(Debug, Clone, PartialEq)]
pub enum MediaResponse {
    Image(ImageResponse),
    Speech(SpeechResponse),
    Video(VideoResponse),
    Transcription(TranscriptionResponse),
}

/// The `finish`-event collection over a completed event stream, per
/// modality (image.ts:156-168, speech.ts:155-169, video.ts:149-161).
pub enum CollectedResponse {
    /// No `finish` event reached the terminal: the stream is defective
    /// (TS `Effect.die`; the driver surfaces the protocol's own failure).
    Incomplete,
    Image(ImageResponse),
    Speech(SpeechResponse),
    Video(VideoResponse),
    Transcription(TranscriptionResponse),
}

/// `collectResponse` for image streams (image.ts:156-168).
#[must_use]
pub fn collect_image_response(events: &[ImageEvent]) -> CollectedResponse {
    let Some(ImageEvent::Finish { usage, notices, provider_metadata }) =
        events.iter().rev().find(|event| matches!(event, ImageEvent::Finish { .. }))
    else {
        return CollectedResponse::Incomplete;
    };
    let images = events
        .iter()
        .filter_map(|event| match event {
            ImageEvent::Image { image, .. } => Some(image.clone()),
            _ => None,
        })
        .collect();
    CollectedResponse::Image(ImageResponse {
        images,
        usage: usage.clone(),
        notices: notices.clone(),
        provider_metadata: provider_metadata.clone(),
    })
}

/// `collectResponse` for speech streams (speech.ts:155-169).
#[must_use]
pub fn collect_speech_response(events: &[SpeechEvent]) -> CollectedResponse {
    let Some(SpeechEvent::Finish { audio, usage, notices, provider_metadata }) =
        events.iter().rev().find(|event| matches!(event, SpeechEvent::Finish { .. }))
    else {
        return CollectedResponse::Incomplete;
    };
    let timestamps: Vec<SpeechTimestamp> = events
        .iter()
        .flat_map(|event| match event {
            SpeechEvent::Timestamps { items } => items.clone(),
            _ => Vec::new(),
        })
        .collect();
    CollectedResponse::Speech(SpeechResponse {
        audio: audio.clone(),
        timestamps: if timestamps.is_empty() { None } else { Some(timestamps) },
        usage: usage.clone(),
        notices: notices.clone(),
        provider_metadata: provider_metadata.clone(),
    })
}

/// `collectResponse` for video streams (video.ts:149-161).
#[must_use]
pub fn collect_video_response(events: &[VideoEvent]) -> CollectedResponse {
    let Some(VideoEvent::Finish { usage, notices, provider_metadata }) =
        events.iter().rev().find(|event| matches!(event, VideoEvent::Finish { .. }))
    else {
        return CollectedResponse::Incomplete;
    };
    let videos = events
        .iter()
        .filter_map(|event| match event {
            VideoEvent::Video { video, .. } => Some(video.clone()),
            _ => None,
        })
        .collect();
    CollectedResponse::Video(VideoResponse {
        videos,
        usage: usage.clone(),
        notices: notices.clone(),
        provider_metadata: provider_metadata.clone(),
    })
}

/// A queued route's first observation plus its serializable token
/// (media-protocol.ts:86-89).
#[derive(Debug, Clone, PartialEq)]
pub struct Started<T> {
    pub token: T,
    pub snapshot: Snapshot,
}

/// Follow-up call inputs (media-protocol.ts:96-100): the decoded token and
/// the auth headers the route sent, so a protocol can attach them to
/// output URLs that require the same credentials to download (Veo).
/// `materialize` is represented by the driver (downloading outputs is a
/// route-layer concern).
pub struct PollContext<'a, T> {
    pub token: &'a T,
    pub auth: &'a std::collections::BTreeMap<String, String>,
}

/// The error a queued protocol's `decode_start/status/result` returns.
pub type DecodeResult<T> = Result<T, AiError>;

// ---------------------------------------------------------------------------
// Unsupported-field presence (route/media.ts:423-448)
// ---------------------------------------------------------------------------

/// Whether one declared-unsupported common field is present on an image
/// request. Arrays count as present only when non-empty; `false` booleans
/// count as present (some mean something when false).
#[must_use]
pub fn image_field_present(request: &ImageRequest, field: &str) -> bool {
    match field {
        "images" => !request.images.is_empty(),
        "mask" => request.mask.is_some(),
        "n" => request.n.is_some(),
        "size" => request.size.is_some(),
        "aspectRatio" => request.aspect_ratio.is_some(),
        "seed" => request.seed.is_some(),
        "format" => request.format.is_some(),
        _ => false,
    }
}

/// Speech-request field presence.
#[must_use]
pub fn speech_field_present(request: &SpeechRequest, field: &str) -> bool {
    match field {
        "voice" => request.voice.is_some(),
        "format" => request.format.is_some(),
        "speed" => request.speed.is_some(),
        "language" => request.language.is_some(),
        "instructions" => request.instructions.is_some(),
        "timestamps" => request.timestamps.is_some(),
        _ => false,
    }
}

/// Video-request field presence.
#[must_use]
pub fn video_field_present(request: &VideoRequest, field: &str) -> bool {
    match field {
        "frames" => request.frames.as_ref().is_some_and(|frames| frames.first.is_some() || frames.last.is_some()),
        "references" => !request.references.is_empty(),
        "video" => request.video.is_some(),
        "durationSeconds" => request.duration_seconds.is_some(),
        "aspectRatio" => request.aspect_ratio.is_some(),
        "resolution" => request.resolution.is_some(),
        "audio" => request.audio.is_some(),
        "n" => request.n.is_some(),
        "seed" => request.seed.is_some(),
        "negativePrompt" => request.negative_prompt.is_some(),
        _ => false,
    }
}

/// Transcription-request field presence.
#[must_use]
pub fn transcription_field_present(request: &TranscriptionRequest, field: &str) -> bool {
    match field {
        "language" => request.language.is_some(),
        "prompt" => request.prompt.is_some(),
        "timestamps" => request.timestamps.is_some(),
        "diarize" => request.diarize.is_some(),
        "speakers" => request.speakers.is_some(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_events_round_trip_the_v2_tags() {
        let event = ImageEvent::Image { index: 0.0, image: crate::schema::media::base64("AQID", "image/png") };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "image");
        assert_eq!(json["index"], 0.0);
        let back: ImageEvent = serde_json::from_value(json).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn speech_finish_serializes_camel_case() {
        let event = SpeechEvent::Finish {
            audio: crate::schema::media::base64("AQID", "audio/mpeg"),
            usage: Some(MediaUsage::Characters { characters: 42.0 }),
            notices: None,
            provider_metadata: None,
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "finish");
        assert_eq!(json["usage"]["type"], "characters");
        assert_eq!(json["usage"]["characters"], 42.0);
    }

    #[test]
    fn collect_image_response_gathers_images_and_finish() {
        let asset = crate::schema::media::base64("AQID", "image/png");
        let events = vec![
            ImageEvent::Image { index: 0.0, image: asset.clone() },
            ImageEvent::Finish {
                usage: Some(MediaUsage::Tokens { input: Some(1.0), output: None, total: None, details: None }),
                notices: None,
                provider_metadata: None,
            },
        ];
        let CollectedResponse::Image(response) = collect_image_response(&events) else {
            panic!("collected");
        };
        assert_eq!(response.images.len(), 1);
        assert_eq!(response.usage, events[1].usage_usage());
        assert!(matches!(collect_image_response(&events[..1]), CollectedResponse::Incomplete));
    }

    impl ImageEvent {
        fn usage_usage(&self) -> Option<MediaUsage> {
            match self {
                Self::Finish { usage, .. } => usage.clone(),
                _ => None,
            }
        }
    }

    #[test]
    fn aspect_ratio_pattern_matches_v2() {
        assert!(crate::schema::media::is_aspect_ratio("16:9"));
        assert!(crate::schema::media::is_aspect_ratio("4.5:3"));
        assert!(!crate::schema::media::is_aspect_ratio("16x9"));
        assert!(!crate::schema::media::is_aspect_ratio("16:9:4"));
        assert!(!crate::schema::media::is_aspect_ratio(":9"));
    }
}
