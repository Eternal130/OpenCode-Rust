//! `openai-transcription` (port of `protocols/openai-transcription.ts`).
//! Stream protocol over multipart uploads. Model families differ:
//! `whisper-*` never streams and returns verbose JSON; `*diarize*` forces
//! `diarized_json` + auto chunking; `gpt-transcribe*` replaces `language`
//! with `languages[]`.

use crate::media::body::{merge_records, Body, Form};
use crate::media::model::{
    transcription_field_present, Addressed, Mode, ResponseContext, TranscriptionEvent,
    TranscriptionRequest, TranscriptionSegment,
};
use crate::media::protocol::{
    Frame, Framing, Identity, MediaProtocolBase, StreamProtocol, TextOutput,
};
use crate::media::utils::{inline_bytes, media_type_extension};
use crate::schema::{AiError, MediaUsage};

pub const ID: &str = "openai-transcription";
pub const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
pub const PATH: &str = "/audio/transcriptions";

pub fn identity() -> Identity {
    Identity::new(ID, "OpenAI Transcription", "openai")
}

/// Model capabilities (openai-transcription.ts:117-132).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    pub stream: bool,
    pub timestamps: &'static [&'static str],
    pub diarize: bool,
    pub language_field: LanguageField,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LanguageField {
    Language,
    Languages,
}

const TRANSCRIBE: Capabilities = Capabilities {
    stream: true,
    timestamps: &[],
    diarize: false,
    language_field: LanguageField::Language,
};

#[must_use]
pub fn capabilities(model: &str) -> Capabilities {
    if model.starts_with("whisper") {
        return Capabilities { stream: false, timestamps: &["segment", "word"], ..TRANSCRIBE };
    }
    if model.contains("diarize") {
        return Capabilities { timestamps: &["segment"], diarize: true, ..TRANSCRIBE };
    }
    // `gpt-transcribe` replaces `language` with `languages[]` and rejects
    // both together.
    if model.starts_with("gpt-transcribe") {
        return Capabilities { language_field: LanguageField::Languages, ..TRANSCRIBE };
    }
    TRANSCRIBE
}

/// `streamsEvents` (openai-transcription.ts:135-136): whisper-1 ignores
/// `stream`, so its stream mode sends a plain request and emits only
/// `finish`.
pub fn streams_events(request: &Addressed<TranscriptionRequest>) -> bool {
    request.mode == Mode::Stream && capabilities(&request.request.model).stream
}

/// `validate` (openai-transcription.ts:138-151).
pub fn validate(
    request: &Addressed<TranscriptionRequest>,
    model: Capabilities,
) -> Result<(), AiError> {
    let id = &request.request.model;
    if request.request.diarize == Some(true) && !model.diarize {
        return Err(identity().unsupported(
            "media.diarize",
            format!("{id} does not diarize; use gpt-4o-transcribe-diarize"),
        ));
    }
    if request.request.prompt.is_some() && model.diarize {
        return Err(
            identity().unsupported("media.prompt", format!("{id} does not accept a prompt"))
        );
    }
    match request.request.timestamps {
        None | Some(crate::media::model::TranscriptionTimestamps::None) => Ok(()),
        Some(requested) => {
            let name = match requested {
                crate::media::model::TranscriptionTimestamps::Segment => "segment",
                crate::media::model::TranscriptionTimestamps::Word => "word",
                crate::media::model::TranscriptionTimestamps::None => unreachable!(),
            };
            if model.timestamps.contains(&name) {
                Ok(())
            } else {
                Err(identity().unsupported(
                    "media.timestamps",
                    format!("{id} does not return {name} timestamps"),
                ))
            }
        }
    }
}

/// `fromRequest` (openai-transcription.ts:163-200). The API detects the
/// audio format from the upload's filename extension.
pub fn body_from(request: &Addressed<TranscriptionRequest>) -> Result<Body, AiError> {
    let model = capabilities(&request.request.model);
    validate(request, model)?;
    let Some(extension) = media_type_extension(&request.request.audio.media_type) else {
        return Err(crate::protocols::shared::invalid_request(format!(
            "OpenAI Transcription cannot name a {} upload; send mp3, mp4, m4a, wav, webm, ogg, or flac audio",
            request.request.audio.media_type
        )));
    };
    let audio = inline_bytes(ID, &request.request.audio)?;
    let response_format = if model.diarize {
        Some("diarized_json")
    } else {
        match request.request.timestamps {
            None | Some(crate::media::model::TranscriptionTimestamps::None) => None,
            Some(_) => Some("verbose_json"),
        }
    };
    let mut form = Form::new();
    form.append_file(
        "file",
        format!("audio.{extension}"),
        request.request.audio.media_type.clone(),
        audio,
    );
    let fields: Vec<(String, serde_json::Value)> = vec![
        ("model".to_string(), serde_json::json!(request.request.model)),
        (
            "language".to_string(),
            crate::media::body::json_opt(
                (model.language_field == LanguageField::Language)
                    .then(|| request.request.language.clone())
                    .flatten(),
            ),
        ),
        (
            "languages".to_string(),
            match (&model.language_field, &request.request.language) {
                (LanguageField::Languages, Some(language)) => serde_json::json!([language]),
                _ => serde_json::Value::Null,
            },
        ),
        ("prompt".to_string(), crate::media::body::json_opt(request.request.prompt.clone())),
        ("response_format".to_string(), crate::media::body::json_opt(response_format)),
        (
            "timestamp_granularities".to_string(),
            match request.request.timestamps {
                Some(crate::media::model::TranscriptionTimestamps::Segment)
                    if response_format == Some("verbose_json") =>
                {
                    serde_json::json!(["segment"])
                }
                Some(crate::media::model::TranscriptionTimestamps::Word)
                    if response_format == Some("verbose_json") =>
                {
                    serde_json::json!(["word"])
                }
                _ => serde_json::Value::Null,
            },
        ),
        // Diarizing audio longer than 30 seconds requires a chunking
        // strategy.
        (
            "chunking_strategy".to_string(),
            if model.diarize { serde_json::json!("auto") } else { serde_json::Value::Null },
        ),
        ("stream".to_string(), crate::media::body::json_flag(streams_events(request))),
    ];
    let fields: Vec<(String, serde_json::Value)> =
        fields.into_iter().filter(|(_, value)| !value.is_null()).collect();
    let overlay = merge_records(&[
        request.request.provider_options.as_ref(),
        request.request.http.as_ref().and_then(|http| http.body.as_ref()),
    ]);
    crate::media::utils::append_fields(
        &mut form,
        fields,
        overlay.as_ref(),
        &[
            "file",
            "model",
            "prompt",
            "language",
            "languages",
            "response_format",
            "timestamp_granularities",
            "stream",
        ],
        Some(crate::media::utils::RepeatArrays::Bracket),
    );
    Ok(Body::multipart(form))
}

pub fn check_unsupported(request: &TranscriptionRequest) -> Result<(), AiError> {
    let present: Vec<&str> = ["speakers"]
        .into_iter()
        .filter(|field| transcription_field_present(request, field))
        .collect();
    identity().reject_unsupported(&present).map_or(Ok(()), Err)
}

/// Parser state (openai-transcription.ts:108-111).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub segments: Vec<TranscriptionSegment>,
    pub transcript: Option<serde_json::Value>,
}

/// `segment` (openai-transcription.ts:206-211).
fn segment_of(value: &serde_json::Value) -> TranscriptionSegment {
    TranscriptionSegment {
        text: value
            .get("text")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string(),
        start_seconds: value.get("start").and_then(serde_json::Value::as_f64).unwrap_or_default(),
        end_seconds: value.get("end").and_then(serde_json::Value::as_f64).unwrap_or_default(),
        speaker: value.get("speaker").and_then(serde_json::Value::as_str).map(str::to_string),
    }
}

/// `onEvent` (openai-transcription.ts:213-229): `OpenAI` may add stream
/// event types; frames outside `EVENT_TYPES` are ignored.
pub fn on_event(state: &mut State, frame: &str) -> Result<Vec<TranscriptionEvent>, AiError> {
    const EVENT_TYPES: &[&str] =
        &["transcript.text.delta", "transcript.text.segment", "transcript.text.done", "error"];
    let output = TextOutput { identity: &identity(), body: frame, http: None };
    let peek = output.decode_frame(frame)?;
    let event_type =
        peek.get("type").and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
    if !EVENT_TYPES.contains(&event_type.as_str()) {
        return Ok(Vec::new());
    }
    if event_type == "error" {
        let message = peek
            .get("message")
            .and_then(serde_json::Value::as_str)
            .or_else(|| peek.pointer("/error/message").and_then(serde_json::Value::as_str))
            .unwrap_or("unknown error");
        return Err(AiError::new(crate::provider_error::classify_provider_failure(
            crate::provider_error::ProviderFailure {
                message: &format!("OpenAI Transcription stream failed: {message}"),
                status: None,
                raw_body: Some(frame),
                data: None,
                http: None,
                retry_after_ms: None,
                rate_limit: None,
            },
        )));
    }
    if event_type == "transcript.text.done" {
        state.transcript = Some(peek);
        return Ok(Vec::new());
    }
    if event_type == "transcript.text.delta" {
        let delta = peek.get("delta").and_then(serde_json::Value::as_str).unwrap_or_default();
        if delta.is_empty() {
            return Ok(Vec::new());
        }
        return Ok(vec![TranscriptionEvent::TextDelta { delta: delta.to_string() }]);
    }
    let next = segment_of(&peek);
    state.segments.push(next.clone());
    Ok(vec![TranscriptionEvent::Segment { segment: next }])
}

/// `usage` (openai-transcription.ts:236-246).
fn usage_of(value: &serde_json::Value) -> Option<MediaUsage> {
    if value.is_null() {
        return None;
    }
    if value.get("type").and_then(serde_json::Value::as_str) == Some("duration") {
        return value
            .get("seconds")
            .and_then(serde_json::Value::as_f64)
            .map(|seconds| MediaUsage::Seconds { seconds });
    }
    Some(MediaUsage::Tokens {
        input: value.get("input_tokens").and_then(serde_json::Value::as_f64),
        output: value.get("output_tokens").and_then(serde_json::Value::as_f64),
        total: value.get("total_tokens").and_then(serde_json::Value::as_f64),
        details: Some(serde_json::from_value(value.clone()).unwrap_or_default()),
    })
}

/// `finish` (openai-transcription.ts:248-262).
pub fn finish(state: &State) -> Result<Vec<TranscriptionEvent>, AiError> {
    let Some(transcript) = &state.transcript else {
        return Err(identity().incomplete());
    };
    let transcript_segments = transcript
        .get("segments")
        .and_then(serde_json::Value::as_array)
        .map(|items| items.iter().map(segment_of).collect::<Vec<_>>());
    let segments = transcript_segments.unwrap_or_else(|| state.segments.clone());
    let words = transcript.get("words").and_then(serde_json::Value::as_array).map(|items| {
        items
            .iter()
            .map(|word| crate::media::model::TranscriptionWord {
                text: word
                    .get("word")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                start_seconds: word
                    .get("start")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or_default(),
                end_seconds: word
                    .get("end")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or_default(),
                speaker: None,
                confidence: None,
            })
            .collect::<Vec<_>>()
    });
    let language = transcript
        .get("language")
        .and_then(serde_json::Value::as_str)
        .or_else(|| transcript.pointer("/languages/0/code").and_then(serde_json::Value::as_str))
        .map(str::to_ascii_lowercase);
    Ok(vec![TranscriptionEvent::Finish(crate::media::model::TranscriptionResponse {
        text: transcript
            .get("text")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        segments: (!segments.is_empty()).then_some(segments),
        words,
        language,
        duration_seconds: transcript.get("duration").and_then(serde_json::Value::as_f64),
        usage: transcript.get("usage").and_then(usage_of),
        notices: None,
        provider_metadata: None,
    })])
}

/// `step` (openai-transcription.ts:231-234): SSE frames or the whole
/// document for `generate`.
pub fn step(state: &mut State, frame: &Frame) -> Result<Vec<TranscriptionEvent>, AiError> {
    match frame {
        Frame::Text(text) => on_event(state, text),
        Frame::Document { document, .. } => {
            let output = TextOutput { identity: &identity(), body: document, http: None };
            state.transcript = Some(output.decode_frame(document)?);
            Ok(Vec::new())
        }
        Frame::Bytes(_) => Ok(Vec::new()),
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct OpenAITranscription;

impl MediaProtocolBase for OpenAITranscription {
    fn identity(&self) -> Identity {
        identity()
    }
    fn unsupported(&self) -> &'static [&'static str] {
        &["speakers"]
    }
    fn base_url(&self) -> String {
        DEFAULT_BASE_URL.to_string()
    }
}

impl StreamProtocol for OpenAITranscription {
    type Request = TranscriptionRequest;
    type Event = TranscriptionEvent;
    type State = State;

    fn body_from(&self, request: &Addressed<TranscriptionRequest>) -> Result<Body, AiError> {
        check_unsupported(&request.request)?;
        body_from(request)
    }

    fn path(&self, _request: &Addressed<TranscriptionRequest>) -> String {
        PATH.to_string()
    }

    fn framing(&self, request: &Addressed<TranscriptionRequest>, _body: &Body) -> Framing {
        if streams_events(request) {
            Framing::Sse
        } else {
            Framing::Document
        }
    }

    fn finish(
        &self,
        state: &Self::State,
        _context: &ResponseContext<TranscriptionRequest>,
    ) -> Result<Vec<TranscriptionEvent>, AiError> {
        finish(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::model::TranscriptionTimestamps;
    use crate::schema::media;
    use serde_json::json;

    fn request(model: &str) -> TranscriptionRequest {
        TranscriptionRequest {
            model: model.into(),
            audio: media::base64("AQID", "audio/mpeg"),
            ..Default::default()
        }
    }

    #[test]
    fn gpt_models_stream_and_use_language() {
        let Body::Multipart(form) =
            body_from(&Addressed::new(request("gpt-4o-transcribe"), Mode::Stream)).unwrap()
        else {
            panic!()
        };
        assert_eq!(form.files(), vec![("file", "audio.mp3", "audio/mpeg")]);
        assert_eq!(form.fields_json()["model"], json!("gpt-4o-transcribe"));
        assert_eq!(form.fields_json()["stream"], json!(true));
        assert!(form.fields_json().get("response_format").is_none());

        let mut with_language = request("gpt-4o-transcribe");
        with_language.language = Some("en".into());
        let Body::Multipart(form) =
            body_from(&Addressed::new(with_language, Mode::Generate)).unwrap()
        else {
            panic!()
        };
        assert_eq!(form.fields_json()["language"], json!("en"));
        assert!(form.fields_json().get("languages").is_none());
    }

    #[test]
    fn gpt_transcribe_uses_languages_array() {
        let mut gpt = request("gpt-transcribe");
        gpt.language = Some("fr".into());
        let Body::Multipart(form) = body_from(&Addressed::new(gpt, Mode::Generate)).unwrap() else {
            panic!()
        };
        assert_eq!(form.fields_json()["languages[]"], json!("fr"));
        assert!(form.fields_json().get("language").is_none());
    }

    #[test]
    fn whisper_ignores_stream_and_takes_verbose_json() {
        let mut whisper = request("whisper-1");
        whisper.timestamps = Some(TranscriptionTimestamps::Word);
        let Body::Multipart(form) = body_from(&Addressed::new(whisper, Mode::Stream)).unwrap()
        else {
            panic!()
        };
        assert!(form.fields_json().get("stream").is_none());
        assert_eq!(form.fields_json()["response_format"], json!("verbose_json"));
        assert_eq!(form.fields_json()["timestamp_granularities[]"], json!("word"));
    }

    #[test]
    fn diarize_models_force_diarized_json_and_chunking() {
        let mut diarize = request("gpt-4o-transcribe-diarize");
        diarize.diarize = Some(true);
        let Body::Multipart(form) = body_from(&Addressed::new(diarize, Mode::Generate)).unwrap()
        else {
            panic!()
        };
        assert_eq!(form.fields_json()["response_format"], json!("diarized_json"));
        assert_eq!(form.fields_json()["chunking_strategy"], json!("auto"));

        let mut plain = request("gpt-4o-transcribe");
        plain.diarize = Some(true);
        let error = body_from(&Addressed::new(plain, Mode::Generate)).unwrap_err();
        assert!(error.message().contains("does not diarize"));
    }

    #[test]
    fn unsupported_extensions_fail_invalid_request() {
        let mut weird = request("gpt-4o-transcribe");
        weird.audio = media::base64("AQID", "audio/weird");
        let error = body_from(&Addressed::new(weird, Mode::Generate)).unwrap_err();
        assert!(error.message().contains("cannot name a audio/weird upload"));
    }

    #[test]
    fn document_step_and_finish_map_the_transcript() {
        let mut state = State::default();
        let document = r#"{"text":"hello world","language":"EN","duration":2.5,"segments":[{"text":" hello","start":0.0,"end":1.0},{"text":" world","start":1.0,"end":2.0,"speaker":"A"}],"words":[{"word":"hello","start":0.0,"end":1.0}],"usage":{"type":"duration","seconds":2.5}}"#;
        step(&mut state, &Frame::Document { document: document.to_string(), requested: None })
            .unwrap();
        let events = finish(&state).unwrap();
        let TranscriptionEvent::Finish(response) = &events[0] else { panic!() };
        assert_eq!(response.text, "hello world");
        assert_eq!(response.language.as_deref(), Some("en"));
        assert_eq!(response.segments.as_ref().unwrap().len(), 2);
        assert_eq!(response.segments.as_ref().unwrap()[1].speaker.as_deref(), Some("A"));
        assert_eq!(response.words.as_ref().unwrap()[0].text, "hello");
        assert_eq!(response.usage, Some(MediaUsage::Seconds { seconds: 2.5 }));
        assert_eq!(response.duration_seconds, Some(2.5));
    }

    #[test]
    fn sse_events_delta_segment_and_error() {
        let mut state = State::default();
        let events =
            on_event(&mut state, r#"{"type":"transcript.text.delta","delta":"he"}"#).unwrap();
        assert!(matches!(&events[0], TranscriptionEvent::TextDelta { delta } if delta == "he"));
        let events = on_event(
            &mut state,
            r#"{"type":"transcript.text.segment","text":" hello","start":0.0,"end":1.0}"#,
        )
        .unwrap();
        assert!(
            matches!(&events[0], TranscriptionEvent::Segment { segment } if segment.text == "hello")
        );
        on_event(&mut state, r#"{"type":"transcript.text.done","text":"hello"}"#).unwrap();
        let error = on_event(&mut state, r#"{"type":"error","message":"bad audio"}"#).unwrap_err();
        assert!(error.message().contains("stream failed: bad audio"));

        let empty = on_event(&mut state, r#"{"type":"unknown.event"}"#).unwrap();
        assert!(empty.is_empty());
    }
}
