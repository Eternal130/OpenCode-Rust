//! Media protocol family (port of the v2 media surface: `image.ts`,
//! `speech.ts`, `video.ts`, `transcription.ts`, `route/media-protocol.ts`
//! + the 23 per-provider protocol modules under
//! `packages/ai/src/protocols/*-{images,video,speech,transcription}.ts`).
//!
//! Layout:
//! - [`model`] — the per-modality request/response/event types and the
//!   queued-generation `Status`/`Snapshot` vocabulary.
//! - [`body`] — JSON / multipart / binary bodies with query parameters.
//! - [`protocol`] — the `InlineProtocol` / `StreamProtocol` /
//!   `QueuedProtocol` seams plus the identity error-mapping contract
//!   (`frameError`, `incomplete`, `unsupported`, `ended`, `pending`,
//!   `contentPolicy`, status/failure tables, `expiringUrl`).
//! - [`utils`] — `MediaInput`, media-type sniffing, `SpeechStream`,
//!   `SpeakerTurns`.
//! - [`image`] / [`video`] / [`speech`] / [`transcription`] — the concrete
//!   provider protocols, one Rust file per TS module.
//!
//! The executor/route composition (`route/media.ts` — auth, polling
//! schedules, upload-before-submit) is the seam a future media-route lane
//! closes; these modules are the protocol layer exactly as v2 defines it.

pub mod body;
pub mod fal_queue;
pub mod image;
pub mod model;
pub mod protocol;
pub mod speech;
pub mod transcription;
pub mod utils;
pub mod video;

pub use body::{Body, Form, FormValue, Query, QueryValue};
pub use model::{
    collect_image_response, collect_speech_response, collect_video_response, is_terminal,
    Addressed, CollectedResponse, DecodeContext, ImageEvent, ImageRequest, ImageResponse,
    MediaRequest, MediaResponse, Mode, ProgressEvent, QueuedEvent, ResponseContext, Snapshot,
    SpeechEvent, SpeechRequest, SpeechResponse, SpeechTimestamp, SpeechVoice, Started, Status,
    TranscriptionEvent, TranscriptionRequest, TranscriptionResponse, TranscriptionSegment,
    TranscriptionTimestamps, TranscriptionWord, VideoEvent, VideoFrames, VideoRequest,
    VideoResponse,
};
pub use protocol::{
    expiring_url, failure, now_millis, status, CancelSpec, Failure, Frame, Framing, Identity,
    InlineProtocol, MediaProtocolBase, QueuedProtocol, StreamProtocol, TextOutput,
};
