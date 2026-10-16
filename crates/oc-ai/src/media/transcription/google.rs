//! `google-transcription` (port of
//! `protocols/google-transcription.ts`). Stream protocol over Gemini
//! generateContent with `audioTranscriptionConfig`; general Gemini models
//! ignore the config and answer the audio conversationally, so non-
//! `transcribe` models fail typed.

use crate::media::body::{merge_records, Body};
use crate::media::model::{
    transcription_field_present, Addressed, Mode, ResponseContext, TranscriptionEvent,
    TranscriptionRequest, TranscriptionWord,
};
use crate::media::protocol::{Frame, Identity, MediaProtocolBase, StreamProtocol};
use crate::media::utils::gemini;
use crate::schema::{AiError, MediaUsage};

pub const ID: &str = "google-transcription";
pub const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

pub fn identity() -> Identity {
    Identity::new(ID, "Google Transcription", "google")
}

/// `fromRequest` (google-transcription.ts:82-111).
pub fn body_from(request: &Addressed<TranscriptionRequest>) -> Result<Body, AiError> {
    // General Gemini models ignore `audioTranscriptionConfig` and answer
    // the audio conversationally.
    if !request.request.model.contains("transcribe") {
        return Err(identity().unsupported(
            "transcription.model",
            format!(
                "{} is not a transcription model; use a transcribe model such as gemini-3.5-transcribe",
                request.request.model
            ),
        ));
    }
    let audio_part = gemini::media_part(ID, &request.request.audio)?;
    let config_base = crate::media::body::record([
        (
            "languageCodes",
            request.request.language.as_ref().map(|language| serde_json::json!([language])),
        ),
        (
            // Parts carry no offsets of their own, so segment times come
            // from word offsets.
            "wordTimestamp",
            (request.request.diarize == Some(true)
                || request.request.timestamps
                    == Some(crate::media::model::TranscriptionTimestamps::Word)
                || request.request.timestamps
                    == Some(crate::media::model::TranscriptionTimestamps::Segment))
            .then(|| serde_json::json!(true)),
        ),
        ("diarization", (request.request.diarize == Some(true)).then(|| serde_json::json!(true))),
    ])
    .unwrap_or_default();
    let config = merge_records(&[
        Some(
            &serde_json::from_value::<serde_json::Map<String, serde_json::Value>>(
                serde_json::json!({ "audioTranscriptionConfig": config_base }),
            )
            .unwrap_or_default(),
        ),
        request.request.provider_options.as_ref(),
    ])
    .unwrap_or_default();
    let merged = merge_records(&[
        Some(
            &serde_json::from_value::<serde_json::Map<String, serde_json::Value>>(
                serde_json::json!({
                    "contents": [{ "role": "user", "parts": [audio_part] }],
                    "generationConfig": config,
                }),
            )
            .unwrap_or_default(),
        ),
        request.request.http.as_ref().and_then(|http| http.body.as_ref()),
    ])
    .unwrap_or_default();
    Ok(Body::json(merged))
}

pub fn check_unsupported(request: &TranscriptionRequest) -> Result<(), AiError> {
    let present: Vec<&str> = ["prompt", "speakers"]
        .into_iter()
        .filter(|field| transcription_field_present(request, field))
        .collect();
    identity().reject_unsupported(&present).map_or(Ok(()), Err)
}

/// Parser state (google-transcription.ts:72-77).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub metadata: gemini::Metadata,
    pub text: String,
    pub segments: Vec<crate::media::model::TranscriptionSegment>,
    pub words: Vec<TranscriptionWord>,
}

/// `Seconds` strings (`"1.5s"`) parse to numbers
/// (google-transcription.ts:42-47).
fn seconds(value: &serde_json::Value) -> Option<f64> {
    let text = value.as_str()?;
    text.strip_suffix('s')?.parse::<f64>().ok()
}

/// `turn` (google-transcription.ts:117-139).
fn turn(
    part: &serde_json::Value,
) -> (String, Vec<TranscriptionWord>, Option<crate::media::model::TranscriptionSegment>) {
    let text = part.get("text").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let speaker_label = part.get("speakerLabel").and_then(|v| v.as_str());
    let words: Vec<TranscriptionWord> = part
        .get("words")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .map(|word| TranscriptionWord {
                    text: word.get("word").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                    start_seconds: word.get("startOffset").and_then(seconds).unwrap_or_default(),
                    end_seconds: word.get("endOffset").and_then(seconds).unwrap_or_default(),
                    speaker: word
                        .get("speakerLabel")
                        .and_then(|v| v.as_str())
                        .or(speaker_label)
                        .map(str::to_string),
                    confidence: None,
                })
                .collect()
        })
        .unwrap_or_default();
    let segment = match (words.first(), words.last()) {
        (Some(first), Some(last)) => Some(crate::media::model::TranscriptionSegment {
            text: text.clone(),
            start_seconds: first.start_seconds,
            end_seconds: last.end_seconds,
            speaker: speaker_label.map(str::to_string),
        }),
        _ => None,
    };
    (text, words, segment)
}

/// `step` (google-transcription.ts:141-165): each part is one whole
/// speaker turn without surrounding whitespace, so turns join with a
/// space.
pub fn step(state: &mut State, frame: &str) -> Result<Vec<TranscriptionEvent>, AiError> {
    let chunk = gemini::decode_chunk(frame)?;
    if let Some(blocked) = gemini::blocked("Google Transcription", &chunk, frame) {
        return Err(blocked);
    }
    let mut texts = Vec::new();
    let mut events = Vec::new();
    for part in chunk
        .pointer("/candidates/0/content/parts")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default()
    {
        let Some(transcription) = part.get("audioTranscription") else { continue };
        let (text, mut words, segment) = turn(transcription);
        let Some(segment) = segment else { continue };
        state.words.append(&mut words);
        state.segments.push(segment.clone());
        texts.push(text);
        events.push(TranscriptionEvent::Segment { segment });
    }
    let joined = texts.into_iter().filter(|text| !text.is_empty()).collect::<Vec<_>>().join(" ");
    let delta = if joined.is_empty() || state.text.is_empty() {
        joined.clone()
    } else {
        format!(" {joined}")
    };
    if state.text.is_empty() && delta.is_empty() {
        if let Some(withheld) = gemini::withheld("Google Transcription", &chunk, frame) {
            return Err(withheld);
        }
    }
    if !delta.is_empty() {
        events.insert(0, TranscriptionEvent::TextDelta { delta: delta.clone() });
    }
    gemini::track(&mut state.metadata, &chunk);
    state.text.push_str(&delta);
    Ok(events)
}

/// `finish` (google-transcription.ts:167-179).
pub fn finish(state: &State) -> Result<Vec<TranscriptionEvent>, AiError> {
    if state.metadata.finish_reason.is_none() {
        return Err(identity().incomplete());
    }
    Ok(vec![TranscriptionEvent::Finish(crate::media::model::TranscriptionResponse {
        text: state.text.clone(),
        segments: (!state.segments.is_empty()).then(|| state.segments.clone()),
        words: (!state.words.is_empty()).then(|| state.words.clone()),
        usage: gemini::usage(&state.metadata.usage).map(|usage| match usage {
            MediaUsage::Tokens { input, output, total, .. } => {
                let details = state.metadata.usage.as_ref().map(|metadata| {
                    serde_json::from_value::<serde_json::Map<String, serde_json::Value>>(
                        serde_json::json!({
                            "promptTokenCount": metadata.prompt_token_count,
                            "candidatesTokenCount": metadata.candidates_token_count,
                            "totalTokenCount": metadata.total_token_count,
                        }),
                    )
                    .unwrap_or_default()
                });
                MediaUsage::Tokens { input, output, total, details }
            }
            other => other,
        }),
        notices: gemini::notices("Google Transcription", &state.metadata),
        provider_metadata: Some(gemini::provider_metadata(&state.metadata)),
        language: None,
        duration_seconds: None,
    })])
}

#[derive(Debug, Clone, Copy, Default)]
pub struct GoogleTranscription;

impl MediaProtocolBase for GoogleTranscription {
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

impl StreamProtocol for GoogleTranscription {
    type Request = TranscriptionRequest;
    type Event = TranscriptionEvent;
    type State = State;

    fn body_from(&self, request: &Addressed<TranscriptionRequest>) -> Result<Body, AiError> {
        check_unsupported(&request.request)?;
        body_from(request)
    }

    fn path(&self, request: &Addressed<TranscriptionRequest>) -> String {
        gemini::path(&request.request.model, request.mode)
    }

    fn framing(
        &self,
        request: &Addressed<TranscriptionRequest>,
        _body: &Body,
    ) -> crate::media::protocol::Framing {
        gemini::framing(request.mode)
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
    use crate::schema::media;
    use serde_json::json;

    fn request() -> TranscriptionRequest {
        TranscriptionRequest {
            model: "gemini-3.5-transcribe".into(),
            audio: media::base64("AQID", "audio/mpeg"),
            ..Default::default()
        }
    }

    #[test]
    fn non_transcribe_models_fail_typed() {
        let mut general = request();
        general.model = "gemini-3-pro".into();
        let error = body_from(&Addressed::new(general, Mode::Stream)).unwrap_err();
        assert!(error.message().contains("is not a transcription model"));
    }

    #[test]
    fn lowers_inline_audio_and_config() {
        let Body::Json { value, .. } =
            body_from(&Addressed::new(request(), Mode::Generate)).unwrap()
        else {
            panic!()
        };
        assert_eq!(
            value["contents"][0]["parts"][0],
            json!({ "inlineData": { "mimeType": "audio/mpeg", "data": "AQID" } })
        );
        assert!(value.get("generationConfig").is_some());

        let mut diarize = request();
        diarize.diarize = Some(true);
        diarize.language = Some("fr".into());
        let Body::Json { value, .. } = body_from(&Addressed::new(diarize, Mode::Generate)).unwrap()
        else {
            panic!()
        };
        assert_eq!(
            value["generationConfig"]["audioTranscriptionConfig"],
            json!({ "languageCodes": ["fr"], "wordTimestamp": true, "diarization": true })
        );
    }

    #[test]
    fn stream_path_and_framing() {
        let addressed = Addressed::new(request(), Mode::Stream);
        assert_eq!(
            GoogleTranscription.path(&addressed),
            "/models/gemini-3.5-transcribe:streamGenerateContent?alt=sse"
        );
        assert_eq!(
            GoogleTranscription.framing(&addressed, &Body::json(Default::default())),
            crate::media::protocol::Framing::Sse
        );
        let addressed = Addressed::new(request(), Mode::Generate);
        assert_eq!(
            GoogleTranscription.path(&addressed),
            "/models/gemini-3.5-transcribe:generateContent"
        );
    }

    #[test]
    fn steps_join_turns_and_finish_collects() {
        let mut state = State::default();
        let events = step(
            &mut state,
            r#"{"candidates":[{"content":{"parts":[{"audioTranscription":{"text":"Hello","speakerLabel":"A","words":[{"word":"Hello","startOffset":"0s","endOffset":"0.5s","speakerLabel":"A"}]}}]},"finishReason":null}]}"#,
        )
        .unwrap();
        assert!(matches!(&events[0], TranscriptionEvent::TextDelta { delta } if delta == "Hello"));
        assert!(
            matches!(&events[1], TranscriptionEvent::Segment { segment } if segment.speaker.as_deref() == Some("A"))
        );

        let events = step(
            &mut state,
            r#"{"candidates":[{"content":{"parts":[{"audioTranscription":{"text":"there","speakerLabel":"B","words":[{"word":"there","startOffset":"0.6s","endOffset":"0.9s","speakerLabel":"B"}]}}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":9,"candidatesTokenCount":20,"totalTokenCount":29},"modelVersion":"gemini-3.5-transcribe"}"#,
        )
        .unwrap();
        assert!(matches!(&events[0], TranscriptionEvent::TextDelta { delta } if delta == " there"));
        assert_eq!(state.text, "Hello there");

        let events = finish(&state).unwrap();
        let TranscriptionEvent::Finish(response) = &events[0] else { panic!() };
        assert_eq!(response.text, "Hello there");
        assert_eq!(response.segments.as_ref().unwrap().len(), 2);
        assert_eq!(response.words.as_ref().unwrap()[1].start_seconds, 0.6);
        assert!(matches!(response.usage, Some(MediaUsage::Tokens { total: Some(29.0), .. })));
    }

    #[test]
    fn prompt_feedback_blocks_the_request() {
        let mut state = State::default();
        let error = step(
            &mut state,
            r#"{"promptFeedback":{"blockReason":"SAFETY","blockReasonMessage":"nope"}}"#,
        )
        .unwrap_err();
        assert!(matches!(error.reason, crate::schema::AiErrorReason::ContentPolicy { .. }));
        assert!(error.message().contains("blocked the request (SAFETY)"));
    }

    #[test]
    fn missing_finish_reason_is_incomplete() {
        let state = State::default();
        let error = finish(&state).unwrap_err();
        assert!(error.message().contains("ended unexpectedly"));
    }
}
