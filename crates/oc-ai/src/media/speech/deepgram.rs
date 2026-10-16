//! `deepgram-speech` (port of `protocols/deepgram-speech.ts`). Stream
//! protocol over raw audio bytes; all options ride as query parameters.
//! Deepgram wraps raw encodings in WAV unless `container` is `none`, and
//! defaults their sample rate per encoding.

use crate::media::body::{merge_records, Body, Query};
use crate::media::model::{
    speech_field_present, Addressed, ResponseContext, SpeechEvent, SpeechRequest,
};
use crate::media::protocol::{Framing, Identity, MediaProtocolBase, StreamProtocol, TextOutput};
use crate::media::utils::speech_stream::PcmEncoding;
use crate::media::utils::{query_from, speech_stream};
use crate::schema::AiError;

pub const ID: &str = "deepgram-speech";
pub const DEFAULT_BASE_URL: &str = "https://api.deepgram.com";
pub const PATH: &str = "/v1/speak";

pub fn identity() -> Identity {
    Identity::new(ID, "Deepgram", "deepgram")
}

/// `FORMATS` (deepgram-speech.ts:40-47).
const FORMATS: &[(&str, &str, Option<&str>)] = &[
    ("mp3", "mp3", None),
    ("wav", "linear16", Some("wav")),
    ("pcm", "linear16", Some("none")),
    ("opus", "opus", None),
    ("flac", "flac", None),
    ("aac", "aac", None),
];

/// `audioFormat` (deepgram-speech.ts:49-55).
fn audio_format(request: &SpeechRequest) -> (Option<String>, Option<String>) {
    let format = request
        .format
        .as_deref()
        .and_then(|format| FORMATS.iter().find(|(known, _, _)| *known == format));
    let provider = request.provider_options.as_ref();
    (
        provider
            .and_then(|options| options.get("encoding"))
            .and_then(|value| value.as_str())
            .map(str::to_string)
            .or_else(|| format.map(|(_, encoding, _)| (*encoding).to_string())),
        provider
            .and_then(|options| options.get("container"))
            .and_then(|value| value.as_str())
            .map(str::to_string)
            .or_else(|| format.and_then(|(_, _, container)| container.map(|c| c.to_string()))),
    )
}

/// `queryParameters` (deepgram-speech.ts:57-67).
fn query_parameters(request: &SpeechRequest) -> Result<Query, AiError> {
    let mut native = request.provider_options.clone().unwrap_or_default();
    native.remove("encoding");
    native.remove("container");
    native.remove("sampleRate");
    native.remove("bitRate");
    let (encoding, container) = audio_format(request);
    let merged = merge_records(&[
        Some(&native),
        Some(
            &crate::media::body::record([
                ("model", Some(serde_json::json!(request.model))),
                ("encoding", encoding.map(|value| serde_json::json!(value))),
                ("container", container.map(|value| serde_json::json!(value))),
                (
                    "sample_rate",
                    request
                        .provider_options
                        .as_ref()
                        .and_then(|options| options.get("sampleRate"))
                        .cloned(),
                ),
                (
                    "bit_rate",
                    request
                        .provider_options
                        .as_ref()
                        .and_then(|options| options.get("bitRate"))
                        .cloned(),
                ),
                ("speed", request.speed.map(|value| serde_json::json!(value))),
            ])
            .unwrap_or_default(),
        ),
    ])
    .unwrap_or_default();
    query_from(ID, &merged)
}

/// `fromRequest` (deepgram-speech.ts:69-86).
pub fn body_from(request: &Addressed<SpeechRequest>) -> Result<Body, AiError> {
    if request.request.timestamps == Some(true) {
        return Err(
            identity().unsupported("media.timestamps", "Deepgram does not return timestamps")
        );
    }
    if let Some(format) = &request.request.format {
        let known = FORMATS.iter().any(|(known, _, _)| known == format);
        let has_encoding = request
            .request
            .provider_options
            .as_ref()
            .and_then(|options| options.get("encoding"))
            .is_some();
        if !known && !has_encoding {
            return Err(identity().unsupported(
                "media.format",
                format!("Deepgram has no encoding for format \"{format}\"; pass providerOptions.encoding"),
            ));
        }
    }
    let base = merge_records(&[
        crate::media::body::record([("text", Some(serde_json::json!(request.request.text)))])
            .as_ref(),
        request.request.http.as_ref().and_then(|http| http.body.as_ref()),
    ])
    .unwrap_or_default();
    Ok(Body::json_with_query(base, query_parameters(&request.request)?))
}

pub fn check_unsupported(request: &SpeechRequest) -> Result<(), AiError> {
    let present: Vec<&str> = ["voice", "language", "instructions"]
        .into_iter()
        .filter(|field| speech_field_present(request, field))
        .collect();
    identity().reject_unsupported(&present).map_or(Ok(()), Err)
}

/// Headerless encodings with their default sample rates
/// (deepgram-speech.ts:93-99).
const HEADERLESS_ENCODINGS: &[(&str, PcmEncoding, f64)] = &[
    ("linear16", PcmEncoding::pcm_s16le, 24_000.0),
    ("mulaw", PcmEncoding::pcm_mulaw, 8_000.0),
    ("alaw", PcmEncoding::pcm_alaw, 8_000.0),
];

/// `finish` (deepgram-speech.ts:101-124): reads content type and billing
/// headers.
#[allow(clippy::too_many_arguments)]
pub fn finish(
    state: &speech_stream::Audio,
    request: &SpeechRequest,
    headers: &std::collections::BTreeMap<String, String>,
) -> Result<Vec<SpeechEvent>, AiError> {
    let header = |name: &str| headers.get(name).map(String::as_str);
    let media_type = header("content-type");
    let (encoding, container) = audio_format(request);
    let headerless = encoding
        .as_deref()
        .and_then(|encoding| HEADERLESS_ENCODINGS.iter().find(|(known, _, _)| *known == encoding));
    let container = container.or_else(|| headerless.map(|_| "wav".to_string()));
    let request_id = header("dg-request-id");
    let model_name = header("dg-model-name");
    let provider_metadata = (request_id.is_some() || model_name.is_some()).then(|| {
        let mut deepgram = std::collections::BTreeMap::new();
        deepgram.insert("requestId".to_string(), serde_json::json!(request_id));
        deepgram.insert("modelName".to_string(), serde_json::json!(model_name));
        let mut metadata = crate::schema::ProviderMetadata::new();
        metadata.insert("deepgram".to_string(), deepgram);
        metadata
    });
    match (container.as_deref(), headerless) {
        (Some("none"), Some((_, pcm_encoding, default_rate))) => {
            let requested_rate = request
                .provider_options
                .as_ref()
                .and_then(|options| options.get("sampleRate"))
                .and_then(|value| value.as_f64());
            let rate = speech_stream::sample_rate(media_type.as_deref())
                .or(requested_rate)
                .unwrap_or(*default_rate);
            speech_stream::finish(
                &identity(),
                state,
                speech_stream::pcm(*pcm_encoding, Some(rate), media_type),
                speech_stream::header_usage("characters", header("dg-char-count")),
                None,
                provider_metadata,
                None,
            )
        }
        _ => {
            // Deepgram's default encoding is MP3; WAV is a container
            // around any encoding.
            let info_format = match container.as_deref() {
                Some("wav") => "wav".to_string(),
                _ => encoding.unwrap_or_else(|| "mp3".to_string()),
            };
            speech_stream::finish(
                &identity(),
                state,
                speech_stream::Output {
                    media_type: media_type.map(str::to_string),
                    info: Some(crate::schema::media::Info {
                        format: Some(info_format),
                        ..Default::default()
                    }),
                },
                speech_stream::header_usage("characters", header("dg-char-count")),
                None,
                provider_metadata,
                None,
            )
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DeepgramSpeech;

impl MediaProtocolBase for DeepgramSpeech {
    fn identity(&self) -> Identity {
        identity()
    }
    fn unsupported(&self) -> &'static [&'static str] {
        &["voice", "language", "instructions"]
    }
    fn base_url(&self) -> String {
        DEFAULT_BASE_URL.to_string()
    }
}

impl StreamProtocol for DeepgramSpeech {
    type Request = SpeechRequest;
    type Event = SpeechEvent;
    type State = speech_stream::Audio;

    fn body_from(&self, request: &Addressed<SpeechRequest>) -> Result<Body, AiError> {
        check_unsupported(&request.request)?;
        body_from(request)
    }

    fn path(&self, _request: &Addressed<SpeechRequest>) -> String {
        PATH.to_string()
    }

    fn framing(&self, _request: &Addressed<SpeechRequest>, _body: &Body) -> Framing {
        Framing::Bytes
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
        finish(state, &context.request.request, &headers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::model::Mode;
    use serde_json::json;

    fn request() -> SpeechRequest {
        SpeechRequest {
            model: "aura-2-thalia-en".into(),
            text: "hello".into(),
            ..Default::default()
        }
    }

    #[test]
    fn query_carries_model_and_default_mp3() {
        let Body::Json { value, query } =
            body_from(&Addressed::new(request(), Mode::Generate)).unwrap()
        else {
            panic!()
        };
        assert_eq!(value["text"], json!("hello"));
        let query = query.expect("query present");
        assert_eq!(
            query.0["model"],
            crate::media::body::QueryValue::One("aura-2-thalia-en".to_string())
        );
        assert!(!query.0.contains_key("encoding"));
    }

    #[test]
    fn pcm_format_lowers_linear16_none() {
        let mut speech_request = request();
        speech_request.format = Some("pcm".into());
        let Body::Json { query, .. } =
            body_from(&Addressed::new(speech_request, Mode::Generate)).unwrap()
        else {
            panic!()
        };
        let query = query.unwrap();
        assert_eq!(
            query.0["encoding"],
            crate::media::body::QueryValue::One("linear16".to_string())
        );
        assert_eq!(query.0["container"], crate::media::body::QueryValue::One("none".to_string()));
    }

    #[test]
    fn unknown_format_without_encoding_fails_typed() {
        let mut speech_request = request();
        speech_request.format = Some("ogg".into());
        let error = body_from(&Addressed::new(speech_request, Mode::Generate)).unwrap_err();
        assert!(error.message().contains("no encoding for format \"ogg\""));
    }

    #[test]
    fn timestamps_rejected_typed() {
        let mut speech_request = request();
        speech_request.timestamps = Some(true);
        let error = body_from(&Addressed::new(speech_request, Mode::Generate)).unwrap_err();
        assert!(error.message().contains("does not return timestamps"));
    }

    #[test]
    fn finish_reads_pcm_headers() {
        let mut state = speech_stream::Audio::default();
        speech_stream::delta(&mut state, vec![1, 2]);
        let mut speech_request = request();
        speech_request.format = Some("pcm".into());
        let mut headers = std::collections::BTreeMap::new();
        headers.insert("content-type".to_string(), "audio/pcm;rate=16000".to_string());
        headers.insert("dg-char-count".to_string(), "33".to_string());
        headers.insert("dg-request-id".to_string(), "req-9".to_string());
        let events = finish(&state, &speech_request, &headers).unwrap();
        let SpeechEvent::Finish { audio, usage, provider_metadata, .. } = &events[0] else {
            panic!()
        };
        assert_eq!(audio.media_type, "audio/pcm;rate=16000");
        assert_eq!(audio.info.as_ref().unwrap().sample_rate, Some(16_000.0));
        assert_eq!(usage, &Some(crate::schema::MediaUsage::Characters { characters: 33.0 }));
        assert_eq!(provider_metadata.as_ref().unwrap()["deepgram"]["requestId"], json!("req-9"));
    }

    #[test]
    fn finish_defaults_container_wav_around_linear16() {
        let mut state = speech_stream::Audio::default();
        speech_stream::delta(&mut state, vec![1, 2]);
        let mut speech_request = request();
        speech_request.format = Some("wav".into());
        let mut headers = std::collections::BTreeMap::new();
        headers.insert("Content-Type".to_string(), "audio/wav".to_string());
        let events = finish(&state, &speech_request, &headers).unwrap();
        let SpeechEvent::Finish { audio, .. } = &events[0] else { panic!() };
        assert_eq!(audio.info.as_ref().unwrap().format.as_deref(), Some("wav"));
    }
}
