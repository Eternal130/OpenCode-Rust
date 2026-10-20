//! `google-speech` (port of `protocols/google-speech.ts`). Stream
//! protocol over Gemini TTS `generateContent` / `streamGenerateContent`:
//! response audio arrives as base64 `inlineData` parts; only
//! `gemini-3.1-*-tts` and later stream.

use crate::media::body::{merge_records, Body};
use crate::media::model::{
    speech_field_present, Addressed, ResponseContext, SpeechEvent, SpeechRequest,
};
use crate::media::protocol::{Frame, Framing, Identity, MediaProtocolBase, StreamProtocol};
use crate::media::utils::gemini;
use crate::media::utils::speech_stream::{self, Audio};
use crate::route::auth::{self, Definition as AuthDefinition, Secret};
use crate::route::transport::SharedTransport;
use serde_json::Value;

use crate::schema::{AiError, HttpOptions};

pub const ID: &str = "google-speech";
pub const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";
/// `DEFAULT_SAMPLE_RATE` (google-speech.ts:11).
pub const DEFAULT_SAMPLE_RATE: f64 = 24_000.0;

pub fn identity() -> Identity {
    Identity::new(ID, "Google Speech", "google")
}

/// Gemini 3.8 TTS unary requests answer WAV regardless of the requested
/// format (google-speech.ts:62-66).
fn is_gemini_38_tts(model: &str) -> bool {
    let rest = model.strip_prefix("gemini-3.8-").unwrap_or(model);
    let tts = rest.strip_suffix("-tts").unwrap_or(rest);
    tts != rest && !tts.is_empty() && !tts.contains('-')
}

/// Parser state (google-speech.ts:48-52): the audio accumulator, the
/// generate-content chunk metadata, and the first audio part's media
/// type.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub audio: Audio,
    pub metadata: gemini::Metadata,
    pub mime_type: Option<String>,
}

/// `fromRequest` (google-speech.ts:58-91).
pub fn body_from(request: &Addressed<SpeechRequest>) -> Result<Body, AiError> {
    // Not in `unsupported`: that list would also reject `timestamps:
    // false`, which asks for nothing.
    if request.request.timestamps == Some(true) {
        return Err(
            identity().unsupported("media.timestamps", "Google Speech does not return timestamps")
        );
    }
    if request.request.format.as_deref() == Some("pcm")
        && request.mode == crate::media::model::Mode::Generate
        && is_gemini_38_tts(&request.request.model)
    {
        return Err(identity().unsupported(
            "media.format",
            "Google Speech returns WAV by default for Gemini 3.8 TTS unary requests; omit the format to accept it",
        ));
    }
    if let Some(format) = &request.request.format {
        if format != "pcm" {
            return Err(identity().unsupported(
                "media.format",
                "Google Speech only accepts raw PCM as an explicit format; omit it to accept the provider's default output",
            ));
        }
    }
    let voice_name = speech_stream::voice_id(&request.request.voice);
    // TS drops `undefined` keys; build the maps with conditional inserts
    // so the wire body never carries null-valued keys the TS encoder omits.
    let mut speech_config = serde_json::Map::new();
    if let Some(voice) = voice_name {
        speech_config.insert(
            "voiceConfig".to_string(),
            serde_json::json!({ "prebuiltVoiceConfig": { "voiceName": voice } }),
        );
    }
    if let Some(language) = &request.request.language {
        speech_config.insert("languageCode".to_string(), serde_json::json!(language));
    }
    let mut generation_config = serde_json::Map::new();
    generation_config.insert("responseModalities".to_string(), serde_json::json!(["AUDIO"]));
    generation_config.insert("speechConfig".to_string(), Value::Object(speech_config));
    let generation_config = merge_records(&[
        Some(generation_config).as_ref(),
        request.request.provider_options.as_ref(),
    ])
    .unwrap_or_default();
    let mut contents = serde_json::Map::new();
    contents.insert("role".to_string(), serde_json::json!("user"));
    contents.insert("parts".to_string(), serde_json::json!([{ "text": request.request.text }]));
    let base = serde_json::json!({
        "contents": [Value::Object(contents)],
        "generationConfig": Value::Object(generation_config),
    });
    let merged = merge_records(&[
        Some(
            serde_json::from_value::<crate::schema::options::JsonRecord>(base).unwrap_or_default(),
        )
        .as_ref(),
        request.request.http.as_ref().and_then(|http| http.body.as_ref()),
    ])
    .unwrap_or_default();
    Ok(Body::json(merged))
}

pub fn check_unsupported(request: &SpeechRequest) -> Result<(), AiError> {
    let present: Vec<&str> = ["instructions", "speed"]
        .into_iter()
        .filter(|field| speech_field_present(request, field))
        .collect();
    identity().reject_unsupported(&present).map_or(Ok(()), Err)
}

/// `step` (google-speech.ts:97-109): every `inlineData` part in the first
/// candidate is one audio delta; blocked prompts and withheld output fail
/// typed.
pub fn step(state: &mut State, frame: &str) -> Result<Vec<SpeechEvent>, AiError> {
    let chunk = gemini::decode_chunk(frame)?;
    if let Some(blocked) = gemini::blocked("Google Speech", &chunk, frame) {
        return Err(blocked);
    }
    let audio: Vec<(String, String)> = chunk
        .pointer("/candidates/0/content/parts")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|part| {
            let inline = part.get("inlineData")?;
            Some((
                inline.get("mimeType").and_then(serde_json::Value::as_str)?.to_string(),
                inline.get("data").and_then(serde_json::Value::as_str)?.to_string(),
            ))
        })
        .collect();
    gemini::track(&mut state.metadata, &chunk);
    if state.mime_type.is_none() {
        state.mime_type = audio.first().map(|(mime, _)| mime.clone());
    }
    let mut events = Vec::new();
    for (_, data) in audio {
        let bytes = crate::protocols::shared::decode_base64(data.as_str()).map_err(|_| {
            identity()
                .frame_error("Google Speech audio contains invalid base64 data", frame.to_string())
        })?;
        events.extend(speech_stream::delta(&mut state.audio, bytes));
    }
    if state.audio.chunks.is_empty() {
        if let Some(withheld) = gemini::withheld("Google Speech", &chunk, frame) {
            return Err(withheld);
        }
    }
    Ok(events)
}

/// `finish` (google-speech.ts:111-129).
pub fn finish(
    state: &State,
    context: &ResponseContext<SpeechRequest>,
) -> Result<Vec<SpeechEvent>, AiError> {
    let Some(finish_reason) = state.metadata.finish_reason.as_deref() else {
        return Err(identity().incomplete());
    };
    let sample_rate =
        speech_stream::sample_rate(state.mime_type.as_deref()).unwrap_or(DEFAULT_SAMPLE_RATE);
    let container = state
        .mime_type
        .as_deref()
        .map(|mime| mime.split(';').next().unwrap_or(mime).to_lowercase());
    let output = if container.as_deref() == Some("audio/wav") {
        speech_stream::container("wav", Some(sample_rate))
    } else {
        speech_stream::pcm(
            speech_stream::PcmEncoding::pcm_s16le,
            Some(sample_rate),
            Some(
                state
                    .mime_type
                    .clone()
                    .unwrap_or_else(|| format!("audio/L16;codec=pcm;rate={sample_rate}"))
                    .as_str(),
            ),
        )
    };
    if context.request.request.format.as_deref() == Some("pcm")
        && output.info.as_ref().and_then(|info| info.format.as_deref()) != Some("pcm")
    {
        return Err(identity().frame_error(
            format!(
                "Google Speech returned {} instead of the requested raw PCM",
                output.info.as_ref().and_then(|info| info.format.as_deref()).unwrap_or_default()
            ),
            String::new(),
        ));
    }
    speech_stream::finish(
        &identity(),
        &state.audio,
        output,
        gemini::usage(&state.metadata.usage),
        gemini::notices("Google Speech", &state.metadata),
        Some(gemini::provider_metadata(&state.metadata)),
        Some(&format!("finish reason: {finish_reason}")),
    )
}

/// The `google-speech` stream protocol (google-speech.ts:135-142).
#[derive(Debug, Clone, Copy, Default)]
pub struct GoogleSpeech;

impl MediaProtocolBase for GoogleSpeech {
    fn identity(&self) -> Identity {
        identity()
    }
    fn unsupported(&self) -> &'static [&'static str] {
        &["instructions", "speed"]
    }
    fn base_url(&self) -> String {
        DEFAULT_BASE_URL.to_string()
    }
}

impl StreamProtocol for GoogleSpeech {
    type Request = SpeechRequest;
    type Event = SpeechEvent;
    type State = State;

    fn body_from(&self, request: &Addressed<SpeechRequest>) -> Result<Body, AiError> {
        check_unsupported(&request.request)?;
        body_from(request)
    }

    /// Only `gemini-3.1-flash-tts-preview` and later stream; earlier TTS
    /// models reject `streamGenerateContent` (google-speech.ts:150).
    fn path(&self, request: &Addressed<SpeechRequest>) -> String {
        gemini::path(&request.request.model, request.mode)
    }

    fn framing(&self, request: &Addressed<SpeechRequest>, _body: &Body) -> Framing {
        gemini::framing(request.mode)
    }

    fn step(&self, state: &mut Self::State, frame: &Frame) -> Result<Vec<SpeechEvent>, AiError> {
        match frame {
            Frame::Text(text) | Frame::Document { document: text, .. } => step(state, text),
            Frame::Bytes(_) => Ok(Vec::new()),
        }
    }

    fn finish(
        &self,
        state: &Self::State,
        context: &ResponseContext<SpeechRequest>,
    ) -> Result<Vec<SpeechEvent>, AiError> {
        finish(state, context)
    }
}

/// Google-facade configuration for [`google_speech_route`].
pub struct GoogleSpeechConfig {
    pub api_key: Option<Secret>,
    pub base_url: Option<String>,
    pub model_http: Option<HttpOptions>,
}

/// The configured `google-speech` media route (port of
/// `Google.configure(...).speech(modelID)`): the `x-goog-api-key` auth
/// chain over the stream route, base URL overridable.
pub fn google_speech_route(
    transport: SharedTransport,
    config: GoogleSpeechConfig,
) -> crate::media::route::StreamMediaRoute<GoogleSpeech> {
    let mut spec = crate::media::route::MediaRouteSpec::new(transport);
    spec.base_url = config.base_url;
    spec.model_http = config.model_http;
    spec.auth = Some(google_speech_auth(config.api_key));
    crate::media::route::StreamMediaRoute::new(
        std::sync::Arc::new(GoogleSpeech),
        spec,
        crate::media::model::collect_speech_response,
    )
}

/// `Auth.optional(apiKey, "apiKey").header("x-goog-api-key")` with the
/// `GOOGLE_GENERATIVE_AI_API_KEY` fallback (providers/google.ts:38-42).
#[must_use]
pub fn google_speech_auth(api_key: Option<Secret>) -> AuthDefinition {
    auth::or_else(&auth::optional(api_key), &auth::config("GOOGLE_GENERATIVE_AI_API_KEY"))
        .header("x-goog-api-key")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::model::{collect_speech_response, CollectedResponse, Mode, SpeechVoice};
    use serde_json::json;

    fn request(model: &str, text: &str) -> SpeechRequest {
        SpeechRequest { model: model.into(), text: text.into(), ..Default::default() }
    }

    #[test]
    fn lowers_the_generate_content_tts_body() {
        let mut speech_request = request("gemini-2.5-flash-preview-tts", "Say: hi");
        speech_request.voice = Some(SpeechVoice::Name("Kore".into()));
        speech_request.language = Some("en-US".into());
        let Body::Json { value, .. } =
            body_from(&Addressed::new(speech_request, Mode::Generate)).unwrap()
        else {
            panic!()
        };
        assert_eq!(
            serde_json::Value::Object(value),
            json!({
                "contents": [{ "role": "user", "parts": [{ "text": "Say: hi" }] }],
                "generationConfig": {
                    "responseModalities": ["AUDIO"],
                    "speechConfig": {
                        "voiceConfig": { "prebuiltVoiceConfig": { "voiceName": "Kore" } },
                        "languageCode": "en-US",
                    }
                }
            })
        );
    }

    #[test]
    fn provider_options_merge_into_generation_config() {
        let mut speech_request = request("gemini-2.5-flash-preview-tts", "x");
        speech_request.provider_options =
            Some([("temperature".to_string(), json!(0.5))].into_iter().collect());
        let Body::Json { value, .. } =
            body_from(&Addressed::new(speech_request, Mode::Generate)).unwrap()
        else {
            panic!()
        };
        let value = serde_json::Value::Object(value);
        assert_eq!(value["generationConfig"]["temperature"], json!(0.5));
        assert_eq!(value["generationConfig"]["responseModalities"], json!(["AUDIO"]));
    }

    #[test]
    fn timestamps_and_foreign_formats_fail_typed() {
        let mut speech_request = request("gemini-2.5-flash-preview-tts", "x");
        speech_request.timestamps = Some(true);
        let error = body_from(&Addressed::new(speech_request, Mode::Generate)).unwrap_err();
        assert!(error.message().contains("does not return timestamps"));

        let mut mp3 = request("gemini-2.5-flash-preview-tts", "x");
        mp3.format = Some("mp3".into());
        let error = body_from(&Addressed::new(mp3, Mode::Generate)).unwrap_err();
        assert!(error.message().contains("only accepts raw PCM"));

        // pcm on a Gemini 3.8 TTS unary request fails (WAV default)
        let mut wav = request("gemini-3.8-flash-tts", "x");
        wav.format = Some("pcm".into());
        let error = body_from(&Addressed::new(wav, Mode::Generate)).unwrap_err();
        assert!(error.message().contains("returns WAV by default"));
        // but the same request streaming is fine
        assert!(body_from(&Addressed::new(
            SpeechRequest {
                model: "gemini-3.8-flash-tts".into(),
                text: "x".into(),
                format: Some("pcm".into()),
                ..Default::default()
            },
            Mode::Stream
        ))
        .is_ok());
    }

    #[test]
    fn instructions_and_speed_reject_as_unsupported() {
        let mut speech_request = request("gemini-2.5-flash-preview-tts", "x");
        speech_request.instructions = Some("be calm".into());
        let error = check_unsupported(&speech_request).unwrap_err();
        assert!(error.message().contains("does not support instructions"));
    }

    #[test]
    fn steps_decode_inline_audio_deltas() {
        let mut state = State::default();
        let frame = r#"{"candidates":[{"content":{"parts":[{"inlineData":{"mimeType":"audio/L16;codec=pcm;rate=24000","data":"AQID"}}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":5,"candidatesTokenCount":9,"totalTokenCount":14}}"#;
        let events = step(&mut state, frame).unwrap();
        assert!(matches!(&events[0], SpeechEvent::AudioDelta { chunk } if chunk == &vec![1, 2, 3]));
        assert_eq!(state.mime_type.as_deref(), Some("audio/L16;codec=pcm;rate=24000"));

        let addressed = Addressed::new(request("m", "x"), Mode::Generate);
        let body = Body::json(serde_json::from_value(json!({})).unwrap());
        let context = ResponseContext {
            request: &addressed,
            body: &body,
            http: &crate::schema::HttpContext {
                url: String::new(),
                status: 200,
                headers: Default::default(),
            },
        };
        let finish_events = finish(&state, &context).unwrap();
        let SpeechEvent::Finish { audio, usage, .. } = &finish_events[0] else { panic!() };
        assert_eq!(audio.media_type, "audio/L16;codec=pcm;rate=24000");
        assert_eq!(audio.info.as_ref().unwrap().encoding.as_deref(), Some("pcm_s16le"));
        assert_eq!(audio.info.as_ref().unwrap().sample_rate, Some(24_000.0));
        assert!(matches!(usage, Some(crate::schema::MediaUsage::Tokens { total: Some(14.0), .. })));
        let CollectedResponse::Speech(response) = collect_speech_response(&finish_events) else {
            panic!()
        };
        assert_eq!(response.audio.media_type, "audio/L16;codec=pcm;rate=24000");
    }

    #[test]
    fn wav_output_against_a_pcm_request_fails_typed() {
        let state = State {
            mime_type: Some("audio/wav".into()),
            metadata: gemini::Metadata { finish_reason: Some("STOP".into()), ..Default::default() },
            audio: Audio { chunks: vec![vec![1]] },
        };
        let addressed = Addressed::new(
            SpeechRequest { format: Some("pcm".into()), ..request("m", "x") },
            Mode::Generate,
        );
        let body = Body::json(serde_json::from_value(json!({})).unwrap());
        let context = ResponseContext {
            request: &addressed,
            body: &body,
            http: &crate::schema::HttpContext {
                url: String::new(),
                status: 200,
                headers: Default::default(),
            },
        };
        let error = finish(&state, &context).unwrap_err();
        assert!(error.message().contains("instead of the requested raw PCM"), "{}", error);
    }
}
