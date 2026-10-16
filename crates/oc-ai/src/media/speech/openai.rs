//! `openai-speech` (port of `protocols/openai-speech.ts`). Stream
//! protocol: `pcm` is raw 24 kHz 16-bit signed little-endian mono; `sse`
//! stream format is unsupported for `tts-1`/`tts-1-hd` which stream the
//! raw audio body instead.

use crate::media::body::{merge_records, Body};
use crate::media::model::{speech_field_present, Addressed, Mode, SpeechEvent, SpeechRequest, ResponseContext};
use crate::media::protocol::{Frame, Framing, Identity, MediaProtocolBase, StreamProtocol, TextOutput};
use crate::media::utils::speech_stream::{self, Audio};
use crate::schema::{AiError, MediaUsage};

pub const ID: &str = "openai-speech";
pub const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
pub const PATH: &str = "/audio/speech";
/// `pcm` is raw 24 kHz, 16-bit signed little-endian mono samples
/// (openai-speech.ts:13).
pub const PCM_SAMPLE_RATE: f64 = 24_000.0;

const FORMATS: &[&str] = &["mp3", "opus", "aac", "flac", "wav", "pcm"];

pub fn identity() -> Identity {
    Identity::new(ID, "OpenAI Speech", "openai")
}

/// `supportsSse` (openai-speech.ts:61).
#[must_use]
pub fn supports_sse(model: &str) -> bool {
    !(model.starts_with("tts-1-hd-") || model == "tts-1-hd" || model.starts_with("tts-1-") || model == "tts-1")
}

/// Parser state (openai-speech.ts:50-54).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub audio: Audio,
    pub done: bool,
    pub usage: Option<MediaUsage>,
}

/// `fromRequest` (openai-speech.ts:65-89).
pub fn body_from(request: &Addressed<SpeechRequest>) -> Result<Body, AiError> {
    // Not in `unsupported`: that list would also reject
    // `timestamps: false`, which asks for nothing.
    if request.request.timestamps == Some(true) {
        return Err(identity().unsupported("media.timestamps", "OpenAI Speech does not return timestamps"));
    }
    if let Some(format) = &request.request.format {
        if !FORMATS.contains(&format.as_str()) {
            return Err(identity().unsupported(
                "media.format",
                format!("OpenAI Speech supports the mp3, opus, aac, flac, wav, and pcm formats, not \"{format}\""),
            ));
        }
    }
    let stream_format = (request.mode == Mode::Stream && supports_sse(&request.request.model))
        .then(|| serde_json::json!("sse"));
    let base = crate::media::body::record([
        ("model", Some(serde_json::json!(request.request.model))),
        ("input", Some(serde_json::json!(request.request.text))),
        (
            "voice",
            request
                .request
                .voice
                .as_ref()
                .map(|voice| serde_json::json!(speech_stream::voice_id(&request.request.voice).unwrap_or_default())),
        ),
        ("instructions", request.request.instructions.clone().map(|value| serde_json::json!(value))),
        ("response_format", request.request.format.clone().map(|value| serde_json::json!(value))),
        ("speed", request.request.speed.map(|value| serde_json::json!(value))),
        ("stream_format", stream_format),
    ]);
    let merged = merge_records(&[
        base.as_ref(),
        request.request.provider_options.as_ref(),
        request.request.http.as_ref().and_then(|http| http.body.as_ref()),
    ])
    .unwrap_or_default();
    Ok(Body::json(merged))
}

pub fn check_unsupported(request: &SpeechRequest) -> Result<(), AiError> {
    let present: Vec<&str> = ["language"]
        .into_iter()
        .filter(|field| speech_field_present(request, field))
        .collect();
    identity().reject_unsupported(&present).map_or(Ok(()), Err)
}

/// `isSse` (openai-speech.ts:95): the merged body selects the framing.
fn is_sse(body: &Body) -> bool {
    matches!(body.json_field("stream_format"), Some(serde_json::Value::String(format)) if format == "sse")
}

/// `onEvent` (openai-speech.ts:97-118): SSE records carry base64 audio
/// deltas and a final usage record.
pub fn on_event(state: &mut State, frame: &str) -> Result<Vec<SpeechEvent>, AiError> {
    let output = TextOutput { identity: &identity(), body: frame, http: None };
    let event = output.decode_frame(frame)?;
    let event_type = event.get("type").and_then(|value| value.as_str()).unwrap_or_default();
    if event_type == "speech.audio.delta" {
        let data = event.get("audio").and_then(|value| value.as_str()).unwrap_or_default();
        let chunk = crate::protocols::shared::decode_base64(data)
            .map_err(|_| identity().frame_error("OpenAI Speech partial audio contains invalid base64 data", frame.to_string()))?;
        return Ok(speech_stream::delta(&mut state.audio, chunk));
    }
    if event_type == "speech.audio.done" {
        state.done = true;
        state.usage = event.get("usage").filter(|value| !value.is_null()).map(|value| MediaUsage::Tokens {
            input: value.get("input_tokens").and_then(|v| v.as_f64()),
            output: value.get("output_tokens").and_then(|v| v.as_f64()),
            total: value.get("total_tokens").and_then(|v| v.as_f64()),
            details: Some(serde_json::from_value(value.clone()).unwrap_or_default()),
        });
    }
    Ok(Vec::new())
}

/// `step` (openai-speech.ts:135-141): SSE frames or raw audio chunks.
pub fn step(state: &mut State, frame: &Frame) -> Result<Vec<SpeechEvent>, AiError> {
    match frame {
        Frame::Text(text) => on_event(state, text),
        Frame::Bytes(bytes) => Ok(speech_stream::delta(&mut state.audio, bytes.clone())),
        Frame::Document { document, .. } => on_event(state, document),
    }
}

/// `finish` (openai-speech.ts:120-129): the sent body reflects
/// `providerOptions` and `http.body` overrides of `format`.
pub fn finish(state: &State, body: &Body) -> Result<Vec<SpeechEvent>, AiError> {
    if is_sse(body) && !state.done {
        return Err(identity().incomplete());
    }
    let sent = body.json_field("response_format").and_then(|value| value.as_str());
    let format = sent.unwrap_or("mp3");
    let output = if format == "pcm" {
        speech_stream::pcm(speech_stream::PcmEncoding::pcm_s16le, Some(PCM_SAMPLE_RATE), None)
    } else {
        speech_stream::container(format, None)
    };
    speech_stream::finish(&identity(), &state.audio, output, state.usage.clone(), None, None, None)
}

#[derive(Debug, Clone, Copy, Default)]
pub struct OpenAISpeech;

impl MediaProtocolBase for OpenAISpeech {
    fn identity(&self) -> Identity {
        identity()
    }
    fn unsupported(&self) -> &'static [&'static str] {
        &["language"]
    }
    fn base_url(&self) -> String {
        DEFAULT_BASE_URL.to_string()
    }
}

impl StreamProtocol for OpenAISpeech {
    type Request = SpeechRequest;
    type Event = SpeechEvent;
    type State = State;

    fn body_from(&self, request: &Addressed<SpeechRequest>) -> Result<Body, AiError> {
        check_unsupported(&request.request)?;
        body_from(request)
    }

    fn path(&self, _request: &Addressed<SpeechRequest>) -> String {
        PATH.to_string()
    }

    fn framing(&self, _request: &Addressed<SpeechRequest>, body: &Body) -> Framing {
        if is_sse(body) { Framing::Sse } else { Framing::Bytes }
    }

    fn finish(&self, state: &Self::State, context: &ResponseContext<SpeechRequest>) -> Result<Vec<SpeechEvent>, AiError> {
        finish(state, context.body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::model::{collect_speech_response, CollectedResponse, SpeechVoice};
    use serde_json::json;

    fn request(model: &str, text: &str) -> SpeechRequest {
        SpeechRequest { model: model.into(), text: text.into(), ..Default::default() }
    }

    #[test]
    fn lowers_the_speech_body() {
        let mut speech_request = request("gpt-4o-mini-tts", "hello");
        speech_request.voice = Some(SpeechVoice::Name("alloy".into()));
        speech_request.format = Some("mp3".into());
        speech_request.speed = Some(1.2);
        let Body::Json { value, .. } = body_from(&Addressed::new(speech_request, Mode::Generate)).unwrap() else {
            panic!()
        };
        assert_eq!(value["model"], json!("gpt-4o-mini-tts"));
        assert_eq!(value["input"], json!("hello"));
        assert_eq!(value["voice"], json!("alloy"));
        assert_eq!(value["response_format"], json!("mp3"));
        assert_eq!(value["speed"], json!(1.2));
        assert!(value.get("stream_format").is_none());
    }

    #[test]
    fn custom_voice_objects_lower_to_their_id() {
        let mut speech_request = request("gpt-4o-mini-tts", "hi");
        speech_request.voice = Some(SpeechVoice::Custom { id: "vr_custom".into() });
        let Body::Json { value, .. } = body_from(&Addressed::new(speech_request, Mode::Generate)).unwrap() else {
            panic!()
        };
        assert_eq!(value["voice"], json!("vr_custom"));
    }

    #[test]
    fn stream_mode_adds_sse_except_on_tts1() {
        let Body::Json { value, .. } = body_from(&Addressed::new(request("gpt-4o-mini-tts", "x"), Mode::Stream)).unwrap()
        else {
            panic!()
        };
        assert_eq!(value["stream_format"], json!("sse"));

        let Body::Json { value, .. } = body_from(&Addressed::new(request("tts-1-hd", "x"), Mode::Stream)).unwrap() else {
            panic!()
        };
        assert!(value.get("stream_format").is_none());
        assert!(!supports_sse("tts-1"));
        assert!(supports_sse("tts-2"));
    }

    #[test]
    fn timestamps_and_unknown_formats_fail_typed() {
        let mut speech_request = request("gpt-4o-mini-tts", "x");
        speech_request.timestamps = Some(true);
        let error = body_from(&Addressed::new(speech_request, Mode::Generate)).unwrap_err();
        assert!(error.message().contains("does not return timestamps"));

        let mut bad_format = request("gpt-4o-mini-tts", "x");
        bad_format.format = Some("ogg".into());
        let error = body_from(&Addressed::new(bad_format, Mode::Generate)).unwrap_err();
        assert!(error.message().contains("not \"ogg\""));
    }

    #[test]
    fn sse_deltas_accumulate_and_finish_describes_pcm() {
        let mut state = State::default();
        let events = on_event(&mut state, r#"{"type":"speech.audio.delta","audio":"AQID"}"#).unwrap();
        assert!(matches!(&events[0], SpeechEvent::AudioDelta { chunk } if chunk == &vec![1, 2, 3]));
        on_event(&mut state, r#"{"type":"speech.audio.done","usage":{"input_tokens":2,"total_tokens":3}}"#).unwrap();
        assert!(state.done);
        let mut body = crate::schema::options::JsonRecord::new();
        body.insert("response_format".to_string(), json!("pcm"));
        let body = Body::json(body);
        let events = finish(&state, &body).unwrap();
        let SpeechEvent::Finish { audio, usage, .. } = &events[0] else { panic!() };
        assert_eq!(audio.media_type, "audio/pcm");
        assert_eq!(audio.info.as_ref().unwrap().sample_rate, Some(PCM_SAMPLE_RATE));
        assert_eq!(audio.info.as_ref().unwrap().channels, Some(1.0));
        assert!(matches!(usage, Some(MediaUsage::Tokens { total: Some(3.0), .. })));
    }

    #[test]
    fn container_formats_keep_their_media_types() {
        let mut state = State::default();
        speech_stream::delta(&mut state.audio, vec![1]);
        let mut body = crate::schema::options::JsonRecord::new();
        body.insert("response_format".to_string(), json!("opus"));
        let events = finish(&state, &Body::json(body)).unwrap();
        let SpeechEvent::Finish { audio, .. } = &events[0] else { panic!() };
        assert_eq!(audio.media_type, "audio/ogg");
        let CollectedResponse::Speech(response) = collect_speech_response(&events) else { panic!() };
        assert_eq!(response.audio.media_type, "audio/ogg");
    }

    #[test]
    fn incomplete_sse_stream_fails() {
        let state = State::default();
        let mut body = crate::schema::options::JsonRecord::new();
        body.insert("stream_format".to_string(), json!("sse"));
        let error = finish(&state, &Body::json(body)).unwrap_err();
        assert!(error.message().contains("ended unexpectedly"));
    }
}
