//! `deepgram-transcription` (port of
//! `protocols/deepgram-transcription.ts`). Inline protocol; URL audio
//! posts `{url}` JSON while inline audio is the raw body with query
//! parameters. Utterances split on pauses, not speakers, so segments
//! split each utterance at speaker changes.

use crate::media::body::{merge_records, Body};
use crate::media::model::{
    transcription_field_present, DecodeContext, TranscriptionRequest, TranscriptionResponse,
};
use crate::media::protocol::{Identity, InlineProtocol, MediaProtocolBase, TextOutput};
use crate::media::utils::{inline_bytes, query_from, speaker_turns};
use crate::schema::{AiError, MediaUsage};

pub const ID: &str = "deepgram-transcription";
pub const DEFAULT_BASE_URL: &str = "https://api.deepgram.com";
pub const PATH: &str = "/v1/listen";

pub fn identity() -> Identity {
    Identity::new(ID, "Deepgram", "deepgram")
}

/// `query` (deepgram-transcription.ts:79-95).
fn query(request: &TranscriptionRequest) -> Result<crate::media::body::Query, AiError> {
    let merged = merge_records(&[
        crate::media::body::record([
            ("model", Some(serde_json::json!(request.model))),
            ("smart_format", Some(serde_json::json!(true))),
            ("language", request.language.clone().map(|value| serde_json::json!(value))),
            // Deepgram assumes English unless asked to detect, unlike the
            // other routes' auto-detection.
            ("detect_language", Some(crate::media::body::json_flag(request.language.is_none()))),
            // `diarize=true` is deprecated in favor of choosing a
            // diarization model.
            (
                "diarize_model",
                if request.diarize == Some(true) {
                    Some(serde_json::json!("latest"))
                } else {
                    None
                },
            ),
            (
                "utterances",
                Some(crate::media::body::json_flag(
                    request.diarize == Some(true)
                        || request.timestamps
                            == Some(crate::media::model::TranscriptionTimestamps::Segment),
                )),
            ),
        ])
        .as_ref(),
        request.provider_options.as_ref(),
    ])
    .unwrap_or_default();
    query_from(ID, &merged)
}

/// `fromRequest` (deepgram-transcription.ts:97-107).
pub fn body_from(request: &TranscriptionRequest) -> Result<Body, AiError> {
    let query = query(request)?;
    if let Some(url) = crate::media::utils::media_url(&request.audio) {
        let base = merge_records(&[
            crate::media::body::record([("url", Some(serde_json::json!(url)))]).as_ref(),
            request.http.as_ref().and_then(|http| http.body.as_ref()),
        ])
        .unwrap_or_default();
        return Ok(Body::json_with_query(base, query));
    }
    if let Some(http) = &request.http {
        if http.body.is_some() {
            return Err(crate::protocols::shared::invalid_request(
                "Deepgram sends inline audio as the raw body, so http.body cannot apply",
            ));
        }
    }
    let audio = inline_bytes(ID, &request.audio)?;
    Ok(Body::binary_with_query(audio, request.audio.media_type.clone(), query))
}

pub fn check_unsupported(request: &TranscriptionRequest) -> Result<(), AiError> {
    let present: Vec<&str> = ["prompt", "speakers"]
        .into_iter()
        .filter(|field| transcription_field_present(request, field))
        .collect();
    identity().reject_unsupported(&present).map_or(Ok(()), Err)
}

fn speaker(value: Option<f64>) -> Option<String> {
    value.map(|value| crate::media::body::normalize_numbers(&serde_json::json!(value)).to_string())
}

fn word_text(word: &serde_json::Value) -> String {
    word.get("punctuated_word")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| word.get("word").and_then(serde_json::Value::as_str).unwrap_or_default())
        .to_string()
}

/// `decodeResponse` (deepgram-transcription.ts:119-161).
pub fn decode(
    output: &TextOutput,
    _context: &DecodeContext<TranscriptionRequest>,
) -> Result<TranscriptionResponse, AiError> {
    let decoded = output.decode_json(output.body)?;
    let channel = decoded.pointer("/results/channels/0");
    let alternative = channel.and_then(|channel| channel.pointer("/alternatives/0"));
    let Some(alternative) = alternative else {
        return Err(output.invalid("Deepgram returned no transcript"));
    };
    let duration = decoded.pointer("/metadata/duration").and_then(serde_json::Value::as_f64);
    let request_id = decoded.pointer("/metadata/request_id").and_then(serde_json::Value::as_str);

    // Utterances split on pauses, not speakers: the v2 diarizer labels a
    // whole utterance with one speaker even when its words change
    // speaker, so segments split each utterance at speaker changes.
    let utterances = decoded
        .pointer("/results/utterances")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut segments = Vec::new();
    for utterance in &utterances {
        let words = utterance.get("words").and_then(serde_json::Value::as_array);
        match words {
            Some(words) if !words.is_empty() => {
                for turn in speaker_turns::group(words, |word| word.get("speaker").cloned()) {
                    let Some(first) = turn.first() else { continue };
                    let Some(last) = turn.last() else { continue };
                    segments.push(crate::media::model::TranscriptionSegment {
                        text: turn.iter().map(word_text).collect::<Vec<_>>().join(" "),
                        start_seconds: first
                            .get("start")
                            .and_then(serde_json::Value::as_f64)
                            .unwrap_or_default(),
                        end_seconds: last
                            .get("end")
                            .and_then(serde_json::Value::as_f64)
                            .unwrap_or_default(),
                        speaker: speaker(first.get("speaker").and_then(serde_json::Value::as_f64)),
                    });
                }
            }
            _ => segments.push(crate::media::model::TranscriptionSegment {
                text: utterance
                    .get("transcript")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                start_seconds: utterance
                    .get("start")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or_default(),
                end_seconds: utterance
                    .get("end")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or_default(),
                speaker: speaker(utterance.get("speaker").and_then(serde_json::Value::as_f64)),
            }),
        }
    }
    let words = alternative.get("words").and_then(serde_json::Value::as_array).map(|words| {
        words
            .iter()
            .map(|word| crate::media::model::TranscriptionWord {
                text: word_text(word),
                start_seconds: word
                    .get("start")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or_default(),
                end_seconds: word
                    .get("end")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or_default(),
                speaker: speaker(word.get("speaker").and_then(serde_json::Value::as_f64)),
                confidence: word.get("confidence").and_then(serde_json::Value::as_f64),
            })
            .collect::<Vec<_>>()
    });
    let provider_metadata = request_id.map(|id| {
        let mut deepgram = std::collections::BTreeMap::new();
        deepgram.insert("requestId".to_string(), serde_json::json!(id));
        let mut metadata = crate::schema::ProviderMetadata::new();
        metadata.insert("deepgram".to_string(), deepgram);
        metadata
    });
    Ok(TranscriptionResponse {
        text: alternative
            .get("transcript")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        segments: (!segments.is_empty()).then_some(segments),
        words,
        language: channel
            .and_then(|channel| channel.get("detected_language"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_ascii_lowercase),
        duration_seconds: duration,
        usage: duration.map(|seconds| MediaUsage::Seconds { seconds }),
        notices: None,
        provider_metadata,
    })
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DeepgramTranscription;

impl MediaProtocolBase for DeepgramTranscription {
    fn identity(&self) -> Identity {
        identity()
    }
    fn unsupported(&self) -> &'static [&'static str] {
        &["prompt", "speakers"]
    }
    fn base_url(&self) -> String {
        DEFAULT_BASE_URL.to_string()
    }
}

impl InlineProtocol for DeepgramTranscription {
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
    use crate::media::body::QueryValue;
    use crate::media::model::TranscriptionTimestamps;
    use crate::schema::media;
    use serde_json::json;

    fn request() -> TranscriptionRequest {
        TranscriptionRequest {
            model: "nova-3".into(),
            audio: media::url("https://x/a.mp3", Some("audio/mpeg".into()), None),
            ..Default::default()
        }
    }

    #[test]
    fn url_audio_posts_json_with_query() {
        let Body::Json { value, query } = body_from(&request()).unwrap() else { panic!() };
        assert_eq!(value["url"], json!("https://x/a.mp3"));
        let query = query.unwrap();
        assert_eq!(query.0["model"], QueryValue::One("nova-3".to_string()));
        assert_eq!(query.0["smart_format"], QueryValue::One("true".to_string()));
        assert_eq!(query.0["detect_language"], QueryValue::One("true".to_string()));
    }

    #[test]
    fn explicit_language_skips_detection_and_diarize_selects_model() {
        let mut diarize = request();
        diarize.language = Some("fr".into());
        diarize.diarize = Some(true);
        diarize.timestamps = Some(TranscriptionTimestamps::Segment);
        let Body::Json { query, .. } = body_from(&diarize).unwrap() else { panic!() };
        let query = query.unwrap();
        assert_eq!(query.0["language"], QueryValue::One("fr".to_string()));
        assert!(!query.0.contains_key("detect_language"));
        assert_eq!(query.0["diarize_model"], QueryValue::One("latest".to_string()));
        assert_eq!(query.0["utterances"], QueryValue::One("true".to_string()));
    }

    #[test]
    fn inline_audio_becomes_the_binary_body() {
        let mut inline = request();
        inline.audio = media::base64("AQID", "audio/wav");
        let Body::Binary { value, content_type, query } = body_from(&inline).unwrap() else {
            panic!()
        };
        assert_eq!(value, vec![1, 2, 3]);
        assert_eq!(content_type, "audio/wav");
        assert!(query.is_some());

        let mut overlay = inline.clone();
        overlay.http = Some(crate::schema::HttpOptions {
            body: Some(crate::schema::options::JsonRecord::new()),
            ..Default::default()
        });
        let error = body_from(&overlay).unwrap_err();
        assert!(error.message().contains("http.body cannot apply"));
    }

    #[test]
    fn decode_splits_utterances_at_speaker_changes() {
        let identity = identity();
        let body_text = r#"{
            "metadata": {"request_id": "r-1", "duration": 3.0},
            "results": {
                "channels": [{
                    "detected_language": "en",
                    "alternatives": [{
                        "transcript": "hello there",
                        "words": [
                            {"word": "hello", "punctuated_word": "Hello", "start": 0.0, "end": 0.5, "confidence": 0.9, "speaker": 0},
                            {"word": "there", "start": 0.6, "end": 1.0, "speaker": 1}
                        ]
                    }]
                }],
                "utterances": [{
                    "transcript": "hello there",
                    "start": 0.0, "end": 1.0,
                    "words": [
                        {"word": "hello", "punctuated_word": "Hello", "start": 0.0, "end": 0.5, "speaker": 0},
                        {"word": "there", "start": 0.6, "end": 1.0, "speaker": 1}
                    ]
                }]
            }
        }"#;
        let output = TextOutput { identity: &identity, body: body_text, http: None };
        let body = Body::json(Default::default());
        let context = DecodeContext { request: &request(), body: &body };
        let response = decode(&output, &context).unwrap();
        assert_eq!(response.text, "hello there");
        assert_eq!(response.language.as_deref(), Some("en"));
        let segments = response.segments.unwrap();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].text, "Hello");
        assert_eq!(segments[0].speaker.as_deref(), Some("0"));
        assert_eq!(segments[1].speaker.as_deref(), Some("1"));
        assert_eq!(segments[1].start_seconds, 0.6);
        assert_eq!(response.words.as_ref().unwrap()[0].text, "Hello");
        assert_eq!(response.usage, Some(MediaUsage::Seconds { seconds: 3.0 }));
        assert_eq!(
            response.provider_metadata.as_ref().unwrap()["deepgram"]["requestId"],
            json!("r-1")
        );
    }
}
