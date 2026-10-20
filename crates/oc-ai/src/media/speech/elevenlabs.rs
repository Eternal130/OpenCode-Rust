//! `elevenlabs-speech` (port of `protocols/elevenlabs-speech.ts`). Stream
//! protocol; `WAV` is served only by the non-streaming endpoints, and
//! character alignment timestamps ride the `/with-timestamps` path.

use crate::media::body::{merge_records, Body};
use crate::media::model::{
    speech_field_present, Addressed, Mode, ResponseContext, SpeechEvent, SpeechRequest,
};
use crate::media::protocol::{
    Frame, Framing, Identity, MediaProtocolBase, StreamProtocol, TextOutput,
};
use crate::media::utils::speech_stream::{self, Audio, PcmEncoding};
use crate::schema::AiError;

pub const ID: &str = "elevenlabs-speech";
pub const DEFAULT_BASE_URL: &str = "https://api.elevenlabs.io";
pub const PATH: &str = "/v1/text-to-speech";

pub fn identity() -> Identity {
    Identity::new(ID, "ElevenLabs", "elevenlabs")
}

/// `OUTPUT_FORMATS` (elevenlabs-speech.ts:90-95).
const OUTPUT_FORMATS: &[(&str, &str)] = &[
    ("mp3", "mp3_44100_128"),
    ("pcm", "pcm_24000"),
    ("wav", "wav_24000"),
    ("opus", "opus_48000_64"),
];

/// Parser state (elevenlabs-speech.ts:84) — the audio accumulator.
pub type State = Audio;

/// `outputFormat` (elevenlabs-speech.ts:98-111).
pub fn output_format(request: &Addressed<SpeechRequest>) -> Result<String, AiError> {
    let format = request
        .request
        .provider_options
        .as_ref()
        .and_then(|options| options.get("outputFormat"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            request
                .request
                .format
                .as_deref()
                .and_then(|format| OUTPUT_FORMATS.iter().find(|(known, _)| *known == format))
                .map(|(_, output)| (*output).to_string())
        })
        .unwrap_or_else(|| "mp3_44100_128".to_string());
    if request
        .request
        .provider_options
        .as_ref()
        .and_then(|options| options.get("outputFormat"))
        .is_none()
        && request.request.format.is_some()
        && OUTPUT_FORMATS
            .iter()
            .find(|(known, _)| Some(*known) == request.request.format.as_deref())
            .is_none()
    {
        return Err(identity().unsupported(
            "media.format",
            format!(
                "ElevenLabs has no default output format for \"{}\"; pass `providerOptions.outputFormat`",
                request.request.format.clone().unwrap_or_default()
            ),
        ));
    }
    if request.mode == Mode::Stream && format.starts_with("wav_") {
        return Err(identity().unsupported(
            "media.format",
            format!("ElevenLabs streams mp3, pcm, opus, ulaw, and alaw but not \"{format}\"; use generate for WAV"),
        ));
    }
    Ok(format)
}

/// `fromRequest` (elevenlabs-speech.ts:113-130).
pub fn body_from(request: &Addressed<SpeechRequest>) -> Result<Body, AiError> {
    if request.request.voice.is_none() {
        return Err(crate::protocols::shared::invalid_request(
            "ElevenLabs requires a voice id; pass it as `voice`",
        ));
    }
    let format = output_format(request)?;
    let mut native = request.request.provider_options.clone().unwrap_or_default();
    native.remove("outputFormat");
    let base = crate::media::body::record([
        ("text", Some(serde_json::json!(request.request.text))),
        ("model_id", Some(serde_json::json!(request.request.model))),
        ("language_code", request.request.language.clone().map(|value| serde_json::json!(value))),
        (
            "voice_settings",
            request.request.speed.map(|speed| serde_json::json!({ "speed": speed })),
        ),
    ]);
    let merged = merge_records(&[
        base.as_ref(),
        Some(&native),
        request.request.http.as_ref().and_then(|http| http.body.as_ref()),
    ])
    .unwrap_or_default();
    let mut query = crate::media::body::Query::new();
    query.insert("output_format", format);
    Ok(Body::json_with_query(merged, query))
}

pub fn check_unsupported(request: &SpeechRequest) -> Result<(), AiError> {
    let present: Vec<&str> =
        ["instructions"].into_iter().filter(|field| speech_field_present(request, field)).collect();
    identity().reject_unsupported(&present).map_or(Ok(()), Err)
}

/// `path` (elevenlabs-speech.ts:132-135).
pub fn path(request: &Addressed<SpeechRequest>) -> String {
    let voice = speech_stream::voice_id(&request.request.voice).unwrap_or_default();
    let encoded: String = url_encode(voice);
    format!(
        "{PATH}/{encoded}{}{}",
        if request.mode == Mode::Stream { "/stream" } else { "" },
        if request.request.timestamps == Some(true) { "/with-timestamps" } else { "" }
    )
}

/// Minimal encodeURIComponent (the voice-id charset is narrow).
fn url_encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// `onRecord` (elevenlabs-speech.ts:141-157): JSON lines carry base64
/// audio plus optional character alignment.
pub fn on_record(state: &mut State, frame: &str) -> Result<Vec<SpeechEvent>, AiError> {
    let output = TextOutput { identity: &identity(), body: frame, http: None };
    let record = output.decode_frame(frame)?;
    let data = record.get("audio_base64").and_then(serde_json::Value::as_str).unwrap_or_default();
    let chunk = crate::protocols::shared::decode_base64(data).map_err(|_| {
        identity().frame_error("ElevenLabs audio contains invalid base64 data", frame.to_string())
    })?;
    let mut events = speech_stream::delta(state, chunk);
    if let Some(alignment) = record.get("alignment").filter(|value| !value.is_null()) {
        let texts: Vec<String> = alignment
            .get("characters")
            .and_then(serde_json::Value::as_array)
            .map(|items| {
                items.iter().filter_map(|item| item.as_str().map(str::to_string)).collect()
            })
            .unwrap_or_default();
        let starts: Vec<f64> = alignment
            .get("character_start_times_seconds")
            .and_then(serde_json::Value::as_array)
            .map(|items| items.iter().filter_map(serde_json::Value::as_f64).collect())
            .unwrap_or_default();
        let ends: Vec<f64> = alignment
            .get("character_end_times_seconds")
            .and_then(serde_json::Value::as_array)
            .map(|items| items.iter().filter_map(serde_json::Value::as_f64).collect())
            .unwrap_or_default();
        events.extend(speech_stream::timestamps(&texts, &starts, &ends));
    }
    Ok(events)
}

/// `step` (elevenlabs-speech.ts:189-199).
pub fn step(state: &mut State, frame: &Frame) -> Result<Vec<SpeechEvent>, AiError> {
    match frame {
        Frame::Text(text) => on_record(state, text),
        Frame::Bytes(bytes) => Ok(speech_stream::delta(state, bytes.clone())),
        Frame::Document { document, .. } => on_record(state, document),
    }
}

/// `describeOutput` (elevenlabs-speech.ts:165-170).
const PCM_CODECS: &[(&str, PcmEncoding)] = &[
    ("pcm", PcmEncoding::pcm_s16le),
    ("ulaw", PcmEncoding::pcm_mulaw),
    ("alaw", PcmEncoding::pcm_alaw),
];

fn describe_output(format: &str) -> speech_stream::Output {
    let (codec, rate) = format.split_once('_').unwrap_or((format, ""));
    let sample_rate = rate.parse::<f64>().ok();
    match PCM_CODECS.iter().find(|(known, _)| *known == codec) {
        Some((_, encoding)) => speech_stream::pcm(*encoding, sample_rate, None),
        None => speech_stream::container(codec, sample_rate),
    }
}

/// `finish` (elevenlabs-speech.ts:172-183): the `character-cost` header is
/// billed credits, not a character count.
pub fn finish(
    state: &State,
    request: &Addressed<SpeechRequest>,
    headers: &std::collections::BTreeMap<String, String>,
) -> Result<Vec<SpeechEvent>, AiError> {
    let format = output_format(request)?;
    let header = |name: &str| headers.get(name).map(String::as_str);
    let provider_metadata = header("request-id").map(|id| {
        let mut elevenlabs = std::collections::BTreeMap::new();
        elevenlabs.insert("requestId".to_string(), serde_json::json!(id));
        let mut metadata = crate::schema::ProviderMetadata::new();
        metadata.insert("elevenlabs".to_string(), elevenlabs);
        metadata
    });
    speech_stream::finish(
        &identity(),
        state,
        describe_output(&format),
        speech_stream::header_usage("credits", header("character-cost")),
        None,
        provider_metadata,
        None,
    )
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ElevenLabsSpeech;

impl MediaProtocolBase for ElevenLabsSpeech {
    fn identity(&self) -> Identity {
        identity()
    }
    fn unsupported(&self) -> &'static [&'static str] {
        &["instructions"]
    }
    fn base_url(&self) -> String {
        DEFAULT_BASE_URL.to_string()
    }
}

impl StreamProtocol for ElevenLabsSpeech {
    type Request = SpeechRequest;
    type Event = SpeechEvent;
    type State = State;

    fn body_from(&self, request: &Addressed<SpeechRequest>) -> Result<Body, AiError> {
        check_unsupported(&request.request)?;
        body_from(request)
    }

    fn path(&self, request: &Addressed<SpeechRequest>) -> String {
        path(request)
    }

    fn framing(&self, request: &Addressed<SpeechRequest>, _body: &Body) -> Framing {
        if request.request.timestamps != Some(true) {
            return Framing::Bytes;
        }
        if request.mode == Mode::Stream {
            Framing::Lines
        } else {
            Framing::Document
        }
    }

    fn finish(
        &self,
        state: &Self::State,
        context: &ResponseContext<SpeechRequest>,
    ) -> Result<Vec<SpeechEvent>, AiError> {
        let headers: std::collections::BTreeMap<String, String> = context
            .http
            .headers
            .iter()
            .map(|(key, value)| (key.to_ascii_lowercase(), value.clone()))
            .collect();
        finish(state, context.request, &headers)
    }

    fn step(&self, state: &mut Self::State, frame: &Frame) -> Result<Vec<SpeechEvent>, AiError> {
        step(state, frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::model::SpeechVoice;
    use serde_json::json;

    fn request() -> SpeechRequest {
        SpeechRequest {
            model: "eleven_flash_v2_5".into(),
            text: "hello".into(),
            voice: Some(SpeechVoice::Name("JBFqnCBsd6RMkjVDRZzb".into())),
            ..Default::default()
        }
    }

    #[test]
    fn body_carries_text_model_and_output_format_query() {
        let Body::Json { value, query } =
            body_from(&Addressed::new(request(), Mode::Generate)).unwrap()
        else {
            panic!()
        };
        assert_eq!(value["text"], json!("hello"));
        assert_eq!(value["model_id"], json!("eleven_flash_v2_5"));
        assert!(value.get("voice_settings").is_none());
        let query = query.unwrap();
        assert_eq!(
            query.0["output_format"],
            crate::media::body::QueryValue::One("mp3_44100_128".to_string())
        );
    }

    #[test]
    fn common_formats_map_and_wav_cannot_stream() {
        let mut pcm = request();
        pcm.format = Some("pcm".into());
        assert_eq!(output_format(&Addressed::new(pcm, Mode::Generate)).unwrap(), "pcm_24000");

        let mut wav = request();
        wav.format = Some("wav".into());
        let error = output_format(&Addressed::new(wav, Mode::Stream)).unwrap_err();
        assert!(error.message().contains("use generate for WAV"));
        assert_eq!(
            output_format(&Addressed::new(request(), Mode::Generate)).unwrap(),
            "mp3_44100_128"
        );
    }

    #[test]
    fn provider_output_format_wins_over_common_field() {
        let mut speech_request = request();
        let mut options = crate::schema::ProviderOptions::new();
        options.insert("outputFormat".to_string(), json!("ulaw_8000"));
        speech_request.provider_options = Some(options);
        assert_eq!(
            output_format(&Addressed::new(speech_request, Mode::Generate)).unwrap(),
            "ulaw_8000"
        );
    }

    #[test]
    fn path_switches_on_stream_and_timestamps() {
        let base = path(&Addressed::new(request(), Mode::Generate));
        assert_eq!(base, "/v1/text-to-speech/JBFqnCBsd6RMkjVDRZzb");
        let stream = path(&Addressed::new(request(), Mode::Stream));
        assert!(stream.ends_with("/stream"));
        let mut timestamps = request();
        timestamps.timestamps = Some(true);
        let with_timestamps = path(&Addressed::new(timestamps, Mode::Generate));
        assert!(with_timestamps.ends_with("/with-timestamps"));
    }

    #[test]
    fn missing_voice_fails() {
        let mut no_voice = request();
        no_voice.voice = None;
        let error = body_from(&Addressed::new(no_voice, Mode::Generate)).unwrap_err();
        assert!(error.message().contains("requires a voice id"));
    }

    #[test]
    fn timestamped_records_emit_alignment() {
        let mut state = State::default();
        let events = on_record(
            &mut state,
            r#"{"audio_base64":"AQID","alignment":{"characters":["h","i"],"character_start_times_seconds":[0.0,0.2],"character_end_times_seconds":[0.1,0.3]}}"#,
        )
        .unwrap();
        assert_eq!(events.len(), 2);
        let SpeechEvent::Timestamps { items } = &events[1] else { panic!() };
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].text, "i");
        assert_eq!(items[1].end_seconds, 0.3);
    }

    #[test]
    fn finish_describes_pcm_output() {
        let mut state = State::default();
        speech_stream::delta(&mut state, vec![4, 5, 6]);
        let mut pcm = request();
        pcm.format = Some("pcm".into());
        let mut headers = std::collections::BTreeMap::new();
        headers.insert("request-id".to_string(), "req-1".to_string());
        headers.insert("character-cost".to_string(), "3".to_string());
        let events = finish(&state, &Addressed::new(pcm, Mode::Generate), &headers).unwrap();
        let SpeechEvent::Finish { audio, usage, provider_metadata, .. } = &events[0] else {
            panic!()
        };
        assert_eq!(audio.media_type, "audio/pcm");
        assert_eq!(audio.info.as_ref().unwrap().sample_rate, Some(24_000.0));
        assert_eq!(usage, &Some(crate::schema::MediaUsage::Credits { credits: 3.0 }));
        assert_eq!(provider_metadata.as_ref().unwrap()["elevenlabs"]["requestId"], json!("req-1"));
    }
}
