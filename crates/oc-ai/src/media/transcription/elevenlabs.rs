//! `elevenlabs-transcription` (port of
//! `protocols/elevenlabs-transcription.ts`) — an inline multipart protocol.
//! speaker turns are the only segments `ElevenLabs` can produce, and turn
//! text keeps the provider's own spacing tokens so languages written
//! without spaces are not re-spaced.

use crate::media::body::{merge_records, Body, Form};
use crate::media::model::{
    transcription_field_present, DecodeContext, TranscriptionRequest, TranscriptionResponse,
    TranscriptionSegment, TranscriptionWord,
};
use crate::media::protocol::{Identity, InlineProtocol, MediaProtocolBase, TextOutput};
use crate::media::utils::{inline_bytes, media_type_extension, speaker_turns, RepeatArrays};
use crate::schema::{AiError, MediaUsage};

pub const ID: &str = "elevenlabs-transcription";
pub const DEFAULT_BASE_URL: &str = "https://api.elevenlabs.io";
pub const PATH: &str = "/v1/speech-to-text";

pub fn identity() -> Identity {
    Identity::new(ID, "ElevenLabs Transcription", "elevenlabs")
}

/// Speaker turns are the only segments `ElevenLabs` can produce, and
/// `num_speakers` only applies to diarization
/// (elevenlabs-transcription.ts:70-71).
pub fn diarizes(request: &TranscriptionRequest) -> bool {
    request.diarize == Some(true)
        || request.timestamps == Some(crate::media::model::TranscriptionTimestamps::Segment)
        || request.speakers.is_some()
}

const RESERVED_FORM_FIELDS: &[&str] = &[
    "file",
    "cloud_storage_url",
    "source_url",
    "model_id",
    "language_code",
    "diarize",
    "num_speakers",
];

/// `validate` (elevenlabs-transcription.ts:83-103).
pub fn validate(
    request: &TranscriptionRequest,
    overlay: &crate::schema::options::JsonRecord,
) -> Result<(), AiError> {
    // Webhook requests return 202 with no transcript; the result arrives
    // at a configured webhook instead.
    if overlay.get("webhook") == Some(&serde_json::json!(true)) {
        return Err(identity().unsupported(
            "transcription.webhook",
            "ElevenLabs Transcription does not deliver to webhooks",
        ));
    }
    // Separate multichannel output replaces the transcript with one
    // transcript per channel.
    if overlay.get("use_multi_channel") == Some(&serde_json::json!(true))
        && overlay.get("multichannel_output_style") != Some(&serde_json::json!("combined"))
    {
        return Err(identity().unsupported(
            "transcription.multichannel",
            "ElevenLabs Transcription returns a single transcript; set multichannel_output_style: \"combined\" to merge channels",
        ));
    }
    if overlay.get("timestamps_granularity") == Some(&serde_json::json!("none"))
        && (request.timestamps == Some(crate::media::model::TranscriptionTimestamps::Word)
            || diarizes(request))
    {
        return Err(identity().unsupported(
            "media.timestamps",
            "ElevenLabs Transcription cannot return word timestamps or speaker turns with timestamps_granularity: \"none\"",
        ));
    }
    Ok(())
}

/// `fromRequest` (elevenlabs-transcription.ts:105-132).
pub fn body_from(request: &TranscriptionRequest) -> Result<Body, AiError> {
    let overlay = merge_records(&[
        request.provider_options.as_ref(),
        request.http.as_ref().and_then(|http| http.body.as_ref()),
    ])
    .unwrap_or_default();
    validate(request, &overlay)?;
    let mut form = Form::new();
    let url = crate::media::utils::media_url(&request.audio);
    if url.is_none() {
        let extension = media_type_extension(&request.audio.media_type);
        let audio = inline_bytes(ID, &request.audio)?;
        form.append_file(
            "file",
            extension.map_or_else(|| "audio".to_string(), |extension| format!("audio.{extension}")),
            request.audio.media_type.clone(),
            audio,
        );
    }
    let fields: Vec<(String, serde_json::Value)> = vec![
        ("model_id".to_string(), serde_json::json!(request.model)),
        // `cloud_storage_url` is deprecated in favor of `source_url`,
        // which accepts any hosted audio or video URL.
        ("source_url".to_string(), crate::media::body::json_opt(url)),
        ("language_code".to_string(), crate::media::body::json_opt(request.language.clone())),
        ("diarize".to_string(), crate::media::body::json_flag(diarizes(request))),
        ("num_speakers".to_string(), crate::media::body::json_opt(request.speakers)),
    ];
    let fields: Vec<(String, serde_json::Value)> =
        fields.into_iter().filter(|(_, value)| !value.is_null()).collect();
    crate::media::utils::append_fields(
        &mut form,
        fields,
        Some(&overlay),
        RESERVED_FORM_FIELDS,
        Some(RepeatArrays::Key),
    );
    Ok(Body::multipart(form))
}

pub fn check_unsupported(request: &TranscriptionRequest) -> Result<(), AiError> {
    let present: Vec<&str> = ["prompt"]
        .into_iter()
        .filter(|field| transcription_field_present(request, field))
        .collect();
    identity().reject_unsupported(&present).map_or(Ok(()), Err)
}

/// One `word`/`spacing`/`audio_event` token (elevenlabs-transcription.ts:47-55).
#[derive(Debug, Clone, PartialEq)]
struct Token {
    text: String,
    type_: String,
    start: Option<f64>,
    end: Option<f64>,
    speaker_id: Option<String>,
    logprob: Option<f64>,
}

fn tokens_of(value: Option<&Vec<serde_json::Value>>) -> Vec<Token> {
    value
        .map(|items| {
            items
                .iter()
                .map(|item| Token {
                    text: item
                        .get("text")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    type_: item
                        .get("type")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    start: item.get("start").and_then(serde_json::Value::as_f64),
                    end: item.get("end").and_then(serde_json::Value::as_f64),
                    speaker_id: item
                        .get("speaker_id")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string),
                    logprob: item.get("logprob").and_then(serde_json::Value::as_f64),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The `speakerTurns` grouping (elevenlabs-transcription.ts:146-164).
fn speaker_turn_segments(tokens: &[Token]) -> Vec<TranscriptionSegment> {
    let relevant: Vec<&Token> =
        tokens.iter().filter(|token| token.type_ == "word" || token.type_ == "spacing").collect();
    speaker_turns::group(&relevant, |token| token.speaker_id.clone())
        .into_iter()
        .filter_map(|turn| {
            let words: Vec<&&Token> = turn
                .iter()
                .filter(|token| {
                    token.start.is_some() && token.end.is_some() && token.type_ == "word"
                })
                .collect();
            let (Some(first), Some(last)) = (words.first(), words.last()) else {
                return None;
            };
            Some(TranscriptionSegment {
                text: turn
                    .iter()
                    .map(|token| token.text.as_str())
                    .collect::<String>()
                    .trim()
                    .to_string(),
                start_seconds: first.start.unwrap_or_default(),
                end_seconds: last.end.unwrap_or_default(),
                speaker: turn.first().and_then(|token| token.speaker_id.clone()),
            })
        })
        .collect()
}

/// `decodeResponse` (elevenlabs-transcription.ts:166-190).
pub fn decode(
    output: &TextOutput,
    context: &DecodeContext<TranscriptionRequest>,
) -> Result<TranscriptionResponse, AiError> {
    let transcript = output.decode_json(output.body)?;
    let tokens = tokens_of(transcript.get("words").and_then(serde_json::Value::as_array));
    let duration = transcript.get("audio_duration_secs").and_then(serde_json::Value::as_f64);
    let transcription_id = transcript.get("transcription_id").and_then(serde_json::Value::as_str);
    let words: Vec<TranscriptionWord> = tokens
        .iter()
        .filter(|token| token.type_ == "word" && token.start.is_some() && token.end.is_some())
        .map(|token| TranscriptionWord {
            text: token.text.clone(),
            start_seconds: token.start.unwrap_or_default(),
            end_seconds: token.end.unwrap_or_default(),
            speaker: token.speaker_id.clone(),
            confidence: token.logprob.map(f64::exp),
        })
        .collect();
    let provider_metadata = transcription_id.map(|id| {
        let mut elevenlabs = std::collections::BTreeMap::new();
        elevenlabs.insert("transcriptionId".to_string(), serde_json::json!(id));
        let mut metadata = crate::schema::ProviderMetadata::new();
        metadata.insert("elevenlabs".to_string(), elevenlabs);
        metadata
    });
    Ok(TranscriptionResponse {
        text: transcript
            .get("text")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        segments: diarizes(context.request)
            .then(|| speaker_turn_segments(&tokens))
            .filter(|segments| !segments.is_empty()),
        words: (!words.is_empty()).then_some(words),
        language: transcript
            .get("language_code")
            .and_then(serde_json::Value::as_str)
            .map(str::to_ascii_lowercase),
        duration_seconds: duration,
        usage: duration.map(|seconds| MediaUsage::Seconds { seconds }),
        notices: None,
        provider_metadata,
    })
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ElevenLabsTranscription;

impl MediaProtocolBase for ElevenLabsTranscription {
    fn identity(&self) -> Identity {
        identity()
    }
    fn unsupported(&self) -> &'static [&'static str] {
        &["prompt"]
    }
    fn base_url(&self) -> String {
        DEFAULT_BASE_URL.to_string()
    }
}

impl InlineProtocol for ElevenLabsTranscription {
    type Request = TranscriptionRequest;
    type Response = TranscriptionResponse;

    fn body_from(&self, request: &TranscriptionRequest) -> Result<Body, AiError> {
        check_unsupported(request)?;
        body_from(request)
    }

    fn path(&self, _request: &TranscriptionRequest) -> String {
        PATH.to_string()
    }

    fn decode(
        &self,
        output: &TextOutput,
        context: &DecodeContext<TranscriptionRequest>,
    ) -> Result<TranscriptionResponse, AiError> {
        decode(output, context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::media;
    use serde_json::json;

    fn request() -> TranscriptionRequest {
        TranscriptionRequest {
            model: "scribe_v1".into(),
            audio: media::base64("AQID", "audio/mpeg"),
            ..Default::default()
        }
    }

    #[test]
    fn inline_audio_uploads_with_named_extension() {
        let Body::Multipart(form) = body_from(&request()).unwrap() else { panic!() };
        assert_eq!(form.files(), vec![("file", "audio.mp3", "audio/mpeg")]);
        assert_eq!(form.fields_json()["model_id"], json!("scribe_v1"));
        assert!(form.fields_json().get("source_url").is_none());
    }

    #[test]
    fn hosted_urls_ride_source_url() {
        let mut hosted = request();
        hosted.audio = media::url("https://x/a.mp4", Some("video/mp4".into()), None);
        let Body::Multipart(form) = body_from(&hosted).unwrap() else { panic!() };
        assert!(form.files().is_empty());
        assert_eq!(form.fields_json()["source_url"], json!("https://x/a.mp4"));
    }

    #[test]
    fn diarize_lands_in_the_form() {
        let mut diarize = request();
        diarize.diarize = Some(true);
        let Body::Multipart(form) = body_from(&diarize).unwrap() else { panic!() };
        assert_eq!(form.fields_json()["diarize"], json!(true));
    }

    #[test]
    fn webhook_and_multichannel_overlays_fail_typed() {
        let mut options = crate::schema::ProviderOptions::new();
        options.insert("webhook".to_string(), json!(true));
        let mut webhook = request();
        webhook.provider_options = Some(options);
        let error = body_from(&webhook).unwrap_err();
        assert!(error.message().contains("does not deliver to webhooks"));

        let mut options = crate::schema::ProviderOptions::new();
        options.insert("use_multi_channel".to_string(), json!(true));
        let mut multichannel = request();
        multichannel.provider_options = Some(options);
        let error = body_from(&multichannel).unwrap_err();
        assert!(error.message().contains("multichannel_output_style"));

        let mut options = crate::schema::ProviderOptions::new();
        options.insert("use_multi_channel".to_string(), json!(true));
        options.insert("multichannel_output_style".to_string(), json!("combined"));
        let mut combined = request();
        combined.provider_options = Some(options);
        assert!(body_from(&combined).is_ok());
    }

    #[test]
    fn decode_groups_speaker_turns_with_spacing_tokens() {
        let identity = identity();
        let body_text = r#"{
            "language_code": "en",
            "text": "Hello there",
            "transcription_id": "tr-1",
            "audio_duration_secs": 1.25,
            "words": [
                {"text": "Hello", "type": "word", "start": 0.0, "end": 0.4, "speaker_id": "spk_0", "logprob": 0.0},
                {"text": " ", "type": "spacing"},
                {"text": "there", "type": "word", "start": 0.5, "end": 0.9, "speaker_id": "spk_1", "logprob": -0.1}
            ]
        }"#;
        let output = TextOutput { identity: &identity, body: body_text, http: None };
        let mut diarize = request();
        diarize.diarize = Some(true);
        let body = Body::json(Default::default());
        let context = DecodeContext { request: &diarize, body: &body };
        let response = decode(&output, &context).unwrap();
        assert_eq!(response.text, "Hello there");
        let segments = response.segments.unwrap();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].text, "Hello");
        assert_eq!(segments[0].speaker.as_deref(), Some("spk_0"));
        assert_eq!(segments[1].text, "there");
        assert_eq!(segments[1].speaker.as_deref(), Some("spk_1"));
        assert_eq!(response.words.as_ref().unwrap()[1].confidence, Some((-0.1f64).exp()));
        assert_eq!(response.usage, Some(MediaUsage::Seconds { seconds: 1.25 }));
        assert_eq!(
            response.provider_metadata.as_ref().unwrap()["elevenlabs"]["transcriptionId"],
            json!("tr-1")
        );

        // Without diarization the segments stay absent.
        let context = DecodeContext { request: &request(), body: &body };
        let response = decode(&output, &context).unwrap();
        assert!(response.segments.is_none());
    }
}
