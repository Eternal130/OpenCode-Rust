//! `cartesia-speech` (port of `protocols/cartesia-speech.ts`). Stream
//! protocol; timestamps exist only on the SSE endpoint, so a `generate`
//! that asks for them collects an SSE stream. Streams and timestamps only
//! support raw PCM.

use crate::media::body::{merge_records, Body};
use crate::media::model::{
    speech_field_present, Addressed, Mode, ResponseContext, SpeechEvent, SpeechRequest,
};
use crate::media::protocol::{
    Frame, Framing, Identity, MediaProtocolBase, StreamProtocol, TextOutput,
};
use crate::media::utils::speech_stream::{self, Audio, PcmEncoding};
use crate::schema::AiError;

pub const ID: &str = "cartesia-speech";
pub const DEFAULT_BASE_URL: &str = "https://api.cartesia.ai";
pub const API_VERSION: &str = "2026-08-14";
pub const BYTES_PATH: &str = "/tts/bytes";
pub const SSE_PATH: &str = "/tts/sse";
const DEFAULT_SAMPLE_RATE: f64 = 44_100.0;
const DEFAULT_BIT_RATE: f64 = 128_000.0;

pub fn identity() -> Identity {
    Identity::new(ID, "Cartesia", "cartesia")
}

/// Parser state (cartesia-speech.ts:65-68).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub audio: Audio,
    pub done: bool,
}

/// `usesSse` (cartesia-speech.ts:74).
fn uses_sse(request: &Addressed<SpeechRequest>) -> bool {
    request.mode == Mode::Stream || request.request.timestamps == Some(true)
}

const CONTAINERS: &[(&str, &str)] = &[("pcm", "raw"), ("wav", "wav"), ("mp3", "mp3")];

/// `outputFormat` (cartesia-speech.ts:78-96) — the resolved container plus
/// the lowered `output_format` body value.
#[derive(Debug, Clone, PartialEq)]
pub struct OutputFormat {
    /// The container name sent to Cartesia.
    pub container: &'static str,
    /// PCM encoding for `raw` containers.
    pub encoding: Option<PcmEncoding>,
    pub sample_rate: f64,
}

pub fn output_format(request: &Addressed<SpeechRequest>) -> Result<OutputFormat, AiError> {
    let sse = uses_sse(request);
    let format = request.request.format.clone().unwrap_or_else(|| {
        if sse {
            "pcm".to_string()
        } else {
            "mp3".to_string()
        }
    });
    let Some((_, container)) = CONTAINERS.iter().find(|(known, _)| *known == format) else {
        return Err(identity().unsupported(
            "media.format",
            format!("Cartesia supports the pcm, wav, and mp3 formats, not \"{format}\""),
        ));
    };
    if sse && *container != "raw" {
        return Err(identity().unsupported(
            "media.format",
            format!("Cartesia streams and timestamps only raw PCM; request format \"pcm\" instead of \"{format}\""),
        ));
    }
    let sample_rate = request
        .request
        .provider_options
        .as_ref()
        .and_then(|options| options.get("sampleRate"))
        .and_then(|value| value.as_f64())
        .unwrap_or(DEFAULT_SAMPLE_RATE);
    if *container == "mp3" {
        return Ok(OutputFormat { container, encoding: None, sample_rate });
    }
    let encoding = request
        .request
        .provider_options
        .as_ref()
        .and_then(|options| options.get("encoding"))
        .and_then(|value| value.as_str())
        .and_then(PcmEncoding::parse)
        .unwrap_or(PcmEncoding::pcm_s16le);
    Ok(OutputFormat { container, encoding: Some(encoding), sample_rate })
}

/// The `output_format` body value Cartesia expects.
fn output_format_body(
    format: &OutputFormat,
    request: &Addressed<SpeechRequest>,
) -> serde_json::Value {
    let bit_rate = request
        .request
        .provider_options
        .as_ref()
        .and_then(|options| options.get("bitRate"))
        .cloned()
        .unwrap_or(serde_json::json!(DEFAULT_BIT_RATE as u64));
    if format.container == "mp3" {
        serde_json::json!({ "container": "mp3", "sample_rate": format.sample_rate as u64, "bit_rate": bit_rate })
    } else {
        let encoding = format
            .encoding
            .map(|encoding| match encoding {
                PcmEncoding::pcm_s16le => "pcm_s16le",
                PcmEncoding::pcm_f32le => "pcm_f32le",
                PcmEncoding::pcm_mulaw => "pcm_mulaw",
                PcmEncoding::pcm_alaw => "pcm_alaw",
            })
            .unwrap_or("pcm_s16le");
        serde_json::json!({ "container": "raw", "encoding": encoding, "sample_rate": format.sample_rate as u64 })
    }
}

/// `fromRequest` (cartesia-speech.ts:98-118).
pub fn body_from(request: &Addressed<SpeechRequest>) -> Result<Body, AiError> {
    let voice = speech_stream::voice_id(&request.request.voice);
    let Some(voice) = voice else {
        return Err(crate::protocols::shared::invalid_request(
            "Cartesia requires a voice id; pass it as `voice`",
        ));
    };
    let format = output_format(request)?;
    let mut native = request.request.provider_options.clone().unwrap_or_default();
    native.remove("sampleRate");
    native.remove("bitRate");
    native.remove("encoding");
    let base = crate::media::body::record([
        ("model_id", Some(serde_json::json!(request.request.model))),
        ("transcript", Some(serde_json::json!(request.request.text))),
        ("voice", Some(serde_json::json!(voice))),
        ("output_format", Some(output_format_body(&format, request))),
        ("language", request.request.language.clone().map(|value| serde_json::json!(value))),
        (
            "generation_config",
            request.request.speed.map(|speed| serde_json::json!({ "speed": speed })),
        ),
        (
            "add_timestamps",
            (request.request.timestamps == Some(true)).then(|| serde_json::json!(true)),
        ),
    ]);
    let merged = merge_records(&[
        base.as_ref(),
        Some(&native),
        request.request.http.as_ref().and_then(|http| http.body.as_ref()),
    ])
    .unwrap_or_default();
    Ok(Body::json(merged))
}

pub fn check_unsupported(request: &SpeechRequest) -> Result<(), AiError> {
    let present: Vec<&str> =
        ["instructions"].into_iter().filter(|field| speech_field_present(request, field)).collect();
    identity().reject_unsupported(&present).map_or(Ok(()), Err)
}

/// `onEvent` (cartesia-speech.ts:124-141). `phoneme_timestamps` and future
/// record types are ignored; stream errors map through
/// `classifyProviderFailure`-style status mapping.
pub fn on_event(state: &mut State, frame: &str) -> Result<Vec<SpeechEvent>, AiError> {
    let output = TextOutput { identity: &identity(), body: frame, http: None };
    let event = output.decode_frame(frame)?;
    let event_type = event.get("type").and_then(|value| value.as_str()).unwrap_or_default();
    if event_type == "chunk" {
        if let Some(data) = event.get("data").and_then(|value| value.as_str()) {
            let chunk = crate::protocols::shared::decode_base64(data).map_err(|_| {
                identity()
                    .frame_error("Cartesia chunk contains invalid base64 data", frame.to_string())
            })?;
            return Ok(speech_stream::delta(&mut state.audio, chunk));
        }
        return Ok(Vec::new());
    }
    if event_type == "timestamps" {
        if let Some(words) = event.get("word_timestamps").filter(|value| !value.is_null()) {
            let texts: Vec<String> = words
                .get("words")
                .and_then(|value| value.as_array())
                .map(|items| {
                    items.iter().filter_map(|item| item.as_str().map(str::to_string)).collect()
                })
                .unwrap_or_default();
            let starts: Vec<f64> = words
                .get("start")
                .and_then(|value| value.as_array())
                .map(|items| items.iter().filter_map(|item| item.as_f64()).collect())
                .unwrap_or_default();
            let ends: Vec<f64> = words
                .get("end")
                .and_then(|value| value.as_array())
                .map(|items| items.iter().filter_map(|item| item.as_f64()).collect())
                .unwrap_or_default();
            return Ok(speech_stream::timestamps(&texts, &starts, &ends));
        }
        return Ok(Vec::new());
    }
    if event_type == "done" {
        state.done = true;
        return Ok(Vec::new());
    }
    if event_type == "error" {
        let title = event.get("title").and_then(|value| value.as_str());
        let message =
            event.get("message").and_then(|value| value.as_str()).unwrap_or("unknown error");
        let status = event.get("status_code").and_then(|value| value.as_u64());
        let text = format!(
            "Cartesia stream failed{}: {message}",
            title.map(|title| format!(" ({title})")).unwrap_or_default()
        );
        return Err(classify_stream_error(text, frame, status));
    }
    Ok(Vec::new())
}

/// The `error` record's `classifyProviderFailure` mapping.
fn classify_stream_error(message: String, frame: &str, status: Option<u64>) -> AiError {
    let classification = match status {
        Some(401 | 403) => "Authentication",
        Some(429) => "RateLimit",
        Some(400..=499) => "InvalidRequest",
        _ => "ProviderInternal",
    };
    let reason = match classification {
        "InvalidRequest" => crate::schema::AiErrorReason::InvalidRequest {
            fields: crate::schema::ReasonFields {
                message,
                body: Some(frame.to_string()),
                http: None,
                cause: None,
            },
            parameter: None,
            classification: None,
        },
        "Authentication" => crate::schema::AiErrorReason::Authentication {
            fields: crate::schema::ReasonFields {
                message,
                body: Some(frame.to_string()),
                http: None,
                cause: None,
            },
        },
        "RateLimit" => crate::schema::AiErrorReason::RateLimit {
            fields: crate::schema::ReasonFields {
                message,
                body: Some(frame.to_string()),
                http: None,
                cause: None,
            },
            retry_after_ms: None,
            rate_limit: None,
        },
        _ => crate::schema::AiErrorReason::ProviderInternal {
            fields: crate::schema::ReasonFields {
                message,
                body: Some(frame.to_string()),
                http: None,
                cause: None,
            },
            retry_after_ms: None,
        },
    };
    AiError::new(reason)
}

/// `step` (cartesia-speech.ts:162-169).
pub fn step(state: &mut State, frame: &Frame) -> Result<Vec<SpeechEvent>, AiError> {
    match frame {
        Frame::Text(text) => on_event(state, text),
        Frame::Bytes(bytes) => Ok(speech_stream::delta(&mut state.audio, bytes.clone())),
        Frame::Document { document, .. } => on_event(state, document),
    }
}

/// `finish` (cartesia-speech.ts:143-156).
pub fn finish(
    state: &State,
    request: &Addressed<SpeechRequest>,
) -> Result<Vec<SpeechEvent>, AiError> {
    if uses_sse(request) && !state.done {
        return Err(identity().incomplete());
    }
    let format = output_format(request)?;
    let output = match format.container {
        "raw" => speech_stream::pcm(
            format.encoding.unwrap_or(PcmEncoding::pcm_s16le),
            Some(format.sample_rate),
            None,
        ),
        other => speech_stream::container(other, Some(format.sample_rate)),
    };
    speech_stream::finish(&identity(), &state.audio, output, None, None, None, None)
}

#[derive(Debug, Clone, Copy, Default)]
pub struct CartesiaSpeech;

impl MediaProtocolBase for CartesiaSpeech {
    fn identity(&self) -> Identity {
        identity()
    }
    fn unsupported(&self) -> &'static [&'static str] {
        &["instructions"]
    }
    fn base_url(&self) -> String {
        DEFAULT_BASE_URL.to_string()
    }
    fn headers(&self) -> Vec<(String, String)> {
        vec![("Cartesia-Version".to_string(), API_VERSION.to_string())]
    }
}

impl StreamProtocol for CartesiaSpeech {
    type Request = SpeechRequest;
    type Event = SpeechEvent;
    type State = State;

    fn body_from(&self, request: &Addressed<SpeechRequest>) -> Result<Body, AiError> {
        check_unsupported(&request.request)?;
        body_from(request)
    }

    fn path(&self, request: &Addressed<SpeechRequest>) -> String {
        if uses_sse(request) {
            SSE_PATH.to_string()
        } else {
            BYTES_PATH.to_string()
        }
    }

    fn framing(&self, request: &Addressed<SpeechRequest>, _body: &Body) -> Framing {
        if uses_sse(request) {
            Framing::Sse
        } else {
            Framing::Bytes
        }
    }

    fn finish(
        &self,
        state: &Self::State,
        context: &ResponseContext<SpeechRequest>,
    ) -> Result<Vec<SpeechEvent>, AiError> {
        finish(state, context.request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::model::SpeechVoice;
    use serde_json::json;

    fn request() -> SpeechRequest {
        SpeechRequest {
            model: "sonic-3".into(),
            text: "hello".into(),
            voice: Some(SpeechVoice::Name("voice-1".into())),
            ..Default::default()
        }
    }

    #[test]
    fn generate_defaults_to_mp3_with_bit_rate() {
        let Body::Json { value, .. } =
            body_from(&Addressed::new(request(), Mode::Generate)).unwrap()
        else {
            panic!()
        };
        assert_eq!(value["model_id"], json!("sonic-3"));
        assert_eq!(value["transcript"], json!("hello"));
        assert_eq!(value["voice"], json!("voice-1"));
        assert_eq!(
            value["output_format"],
            json!({ "container": "mp3", "sample_rate": 44100, "bit_rate": 128000 })
        );
    }

    #[test]
    fn stream_or_timestamps_select_raw_pcm_sse() {
        let mut sse_request = request();
        sse_request.timestamps = Some(true);
        let Body::Json { value, .. } =
            body_from(&Addressed::new(sse_request.clone(), Mode::Generate)).unwrap()
        else {
            panic!()
        };
        assert_eq!(
            value["output_format"],
            json!({ "container": "raw", "encoding": "pcm_s16le", "sample_rate": 44100 })
        );
        assert_eq!(value["add_timestamps"], json!(true));
        assert_eq!(CartesiaSpeech.path(&Addressed::new(sse_request, Mode::Generate)), SSE_PATH);
    }

    #[test]
    fn containers_restricted_on_sse() {
        let mut wav_stream = request();
        wav_stream.format = Some("wav".into());
        let error = body_from(&Addressed::new(wav_stream, Mode::Stream)).unwrap_err();
        assert!(error.message().contains("only raw PCM"));

        let mut flac = request();
        flac.format = Some("flac".into());
        let error = body_from(&Addressed::new(flac, Mode::Generate)).unwrap_err();
        assert!(error.message().contains("not \"flac\""));
    }

    #[test]
    fn missing_voice_fails_invalid_request() {
        let mut no_voice = request();
        no_voice.voice = None;
        let error = body_from(&Addressed::new(no_voice, Mode::Generate)).unwrap_err();
        assert!(error.message().contains("requires a voice id"));
    }

    #[test]
    fn sse_events_carry_chunks_timestamps_and_errors() {
        let mut state = State::default();
        let events = on_event(&mut state, r#"{"type":"chunk","data":"AQID"}"#).unwrap();
        assert!(matches!(&events[0], SpeechEvent::AudioDelta { chunk } if chunk == &vec![1, 2, 3]));

        let events = on_event(
            &mut state,
            r#"{"type":"timestamps","word_timestamps":{"words":["a","b"],"start":[0.1,0.5],"end":[0.4,0.9]}}"#,
        )
        .unwrap();
        let SpeechEvent::Timestamps { items } = &events[0] else { panic!() };
        assert_eq!(items[1].text, "b");
        assert_eq!(items[1].start_seconds, 0.5);

        on_event(&mut state, r#"{"type":"done"}"#).unwrap();
        assert!(state.done);

        let error = on_event(
            &mut state,
            r#"{"type":"error","status_code":500,"title":"boom","message":"it broke"}"#,
        )
        .unwrap_err();
        assert!(error.message().contains("Cartesia stream failed (boom): it broke"));
        assert!(matches!(error.reason, crate::schema::AiErrorReason::ProviderInternal { .. }));
    }

    #[test]
    fn finish_describes_raw_pcm() {
        let mut state = State::default();
        speech_stream::delta(&mut state.audio, vec![9, 9]);
        let events = finish(&state, &Addressed::new(request(), Mode::Stream)).unwrap_err();
        let _ = events; // not done yet -> incomplete
        let mut state = State::default();
        state.done = true;
        speech_stream::delta(&mut state.audio, vec![9, 9]);
        let events = finish(&state, &Addressed::new(request(), Mode::Stream)).unwrap();
        let SpeechEvent::Finish { audio, .. } = &events[0] else { panic!() };
        assert_eq!(audio.info.as_ref().unwrap().sample_rate, Some(44_100.0));
    }
}
