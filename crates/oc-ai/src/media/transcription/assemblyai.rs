//! `assemblyai-transcription` (port of
//! `protocols/assemblyai-transcription.ts`). Queued protocol;
//! `/v2/transcript` only takes a URL, so inline audio is uploaded to
//! `/v2/upload` first (the `prepare` step).

use std::collections::BTreeMap;

use crate::media::body::{merge_records, Body};
use crate::media::model::{
    transcription_field_present, PollContext, Snapshot, Started, Status, TranscriptionRequest,
    TranscriptionResponse,
};
use crate::media::protocol::{
    status as table_status, Identity, MediaProtocolBase, QueuedProtocol, TextOutput,
};
use crate::schema::media;
use crate::schema::{AiError, MediaUsage};

pub const ID: &str = "assemblyai-transcription";
pub const DEFAULT_BASE_URL: &str = "https://api.assemblyai.com";
pub const PATH: &str = "/v2/transcript";
pub const UPLOAD_PATH: &str = "/v2/upload";

pub fn identity() -> Identity {
    Identity::new(ID, "AssemblyAI", "assemblyai")
}

/// The serializable token (assemblyai-transcription.ts:43).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Token {
    pub transcript_id: String,
}

fn status_table() -> BTreeMap<&'static str, Status> {
    [
        ("queued", Status::Queued),
        ("processing", Status::Running),
        ("completed", Status::Completed),
        ("error", Status::Failed),
    ]
    .into_iter()
    .collect()
}

/// `prepare` (assemblyai-transcription.ts:94-101): inline audio is
/// uploaded first; the returned request carries the hosted URL.
pub fn prepare(
    request: &TranscriptionRequest,
    upload_url: &str,
) -> Result<TranscriptionRequest, AiError> {
    if !matches!(request.audio.source, media::Source::Bytes { .. } | media::Source::Base64 { .. }) {
        return Ok(request.clone());
    }
    let mut prepared = request.clone();
    prepared.audio = media::url(upload_url, Some(request.audio.media_type.clone()), None);
    Ok(prepared)
}

/// The upload body (the raw audio bytes).
pub fn upload_body(request: &TranscriptionRequest) -> Result<Body, AiError> {
    Ok(Body::binary(
        crate::media::utils::inline_bytes(ID, &request.audio)?,
        "application/octet-stream",
    ))
}

/// `fromRequest` (assemblyai-transcription.ts:103-124).
pub fn body_from(request: &TranscriptionRequest) -> Result<Body, AiError> {
    let (_, audio_url) = crate::media::utils::media_reference(
        &request.audio,
        Some(&crate::schema::ProviderId::new("assemblyai")),
        "AssemblyAI",
    )?;
    let base = crate::media::body::record([
        ("audio_url", Some(serde_json::json!(audio_url))),
        ("speech_models", Some(serde_json::json!([request.model]))),
        ("language_code", request.language.clone().map(|value| serde_json::json!(value))),
        ("language_detection", Some(crate::media::body::json_flag(request.language.is_none()))),
        ("prompt", request.prompt.clone().map(|value| serde_json::json!(value))),
        (
            // Turn-level `utterances`, the only segments AssemblyAI returns,
            // and `speakers_expected` require speaker labels.
            "speaker_labels",
            Some(crate::media::body::json_flag(
                request.diarize == Some(true)
                    || request.timestamps
                        == Some(crate::media::model::TranscriptionTimestamps::Segment)
                    || request.speakers.is_some(),
            )),
        ),
        ("speakers_expected", request.speakers.map(|speakers| serde_json::json!(speakers))),
    ]);
    let merged = merge_records(&[
        base.as_ref(),
        request.provider_options.as_ref(),
        request.http.as_ref().and_then(|http| http.body.as_ref()),
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

/// `decodeStart` (assemblyai-transcription.ts:132-138).
pub fn decode_start(output: &TextOutput) -> Result<Started<Token>, AiError> {
    let value = output.decode_json(output.body)?;
    let raw = value.get("status").and_then(serde_json::Value::as_str).unwrap_or_default();
    let status = table_status(&status_table(), raw, output)?;
    let id = value.get("id").and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
    Ok(Started {
        snapshot: Snapshot { id: id.clone(), status, progress: None, position: None },
        token: Token { transcript_id: id },
    })
}

/// `decodeStatus` (assemblyai-transcription.ts:140-147).
pub fn decode_status(
    output: &TextOutput,
    context: &PollContext<Token>,
) -> Result<Snapshot, AiError> {
    let value = output.decode_json(output.body)?;
    let raw = value.get("status").and_then(serde_json::Value::as_str).unwrap_or_default();
    Ok(Snapshot {
        id: context.token.transcript_id.clone(),
        status: table_status(&status_table(), raw, output)?,
        progress: None,
        position: None,
    })
}

fn seconds(milliseconds: f64) -> f64 {
    milliseconds / 1000.0
}

/// `decodeResult` (assemblyai-transcription.ts:151-185). Word and
/// utterance times are milliseconds.
pub fn decode_result(
    output: &TextOutput,
    context: &PollContext<Token>,
) -> Result<TranscriptionResponse, AiError> {
    let transcript = output.decode_json(output.body)?;
    let raw = transcript.get("status").and_then(serde_json::Value::as_str).unwrap_or_default();
    let status = table_status(&status_table(), raw, output)?;
    let error = transcript.get("error").and_then(serde_json::Value::as_str);
    if status == Status::Failed {
        return Err(output.ended(
            status,
            format!(
                "AssemblyAI transcription failed{}",
                error.map(|error| format!(": {error}")).unwrap_or_default()
            ),
            crate::media::protocol::Failure::ProviderInternal,
        ));
    }
    if status != Status::Completed {
        return Err(output.pending(&context.token.transcript_id));
    }
    let duration = transcript.get("audio_duration").and_then(serde_json::Value::as_f64);
    let segments =
        transcript.get("utterances").and_then(serde_json::Value::as_array).map(|utterances| {
            utterances
                .iter()
                .map(|utterance| crate::media::model::TranscriptionSegment {
                    text: utterance
                        .get("text")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    start_seconds: utterance
                        .get("start")
                        .and_then(serde_json::Value::as_f64)
                        .map(seconds)
                        .unwrap_or_default(),
                    end_seconds: utterance
                        .get("end")
                        .and_then(serde_json::Value::as_f64)
                        .map(seconds)
                        .unwrap_or_default(),
                    speaker: utterance
                        .get("speaker")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string),
                })
                .collect::<Vec<_>>()
        });
    let words = transcript.get("words").and_then(serde_json::Value::as_array).map(|words| {
        words
            .iter()
            .map(|word| crate::media::model::TranscriptionWord {
                text: word
                    .get("text")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                start_seconds: word
                    .get("start")
                    .and_then(serde_json::Value::as_f64)
                    .map(seconds)
                    .unwrap_or_default(),
                end_seconds: word
                    .get("end")
                    .and_then(serde_json::Value::as_f64)
                    .map(seconds)
                    .unwrap_or_default(),
                speaker: word
                    .get("speaker")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
                confidence: word.get("confidence").and_then(serde_json::Value::as_f64),
            })
            .collect::<Vec<_>>()
    });
    let mut assemblyai = std::collections::BTreeMap::new();
    assemblyai.insert(
        "transcriptId".to_string(),
        serde_json::json!(transcript.get("id").cloned().unwrap_or_default()),
    );
    assemblyai.insert(
        "speechModel".to_string(),
        transcript.get("speech_model_used").filter(|v| !v.is_null()).cloned().unwrap_or_default(),
    );
    let mut metadata = crate::schema::ProviderMetadata::new();
    metadata.insert("assemblyai".to_string(), assemblyai);
    Ok(TranscriptionResponse {
        text: transcript
            .get("text")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        segments,
        words,
        language: transcript
            .get("language_code")
            .and_then(serde_json::Value::as_str)
            .map(str::to_ascii_lowercase),
        duration_seconds: duration,
        usage: duration.map(|seconds| MediaUsage::Seconds { seconds }),
        notices: None,
        provider_metadata: Some(metadata),
    })
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AssemblyAITranscription;

impl MediaProtocolBase for AssemblyAITranscription {
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

impl QueuedProtocol for AssemblyAITranscription {
    type Request = TranscriptionRequest;
    type Response = TranscriptionResponse;
    type Token = Token;

    fn body_from(&self, request: &TranscriptionRequest) -> Result<Body, AiError> {
        check_unsupported(request)?;
        body_from(request)
    }

    fn start_path(&self, _request: &TranscriptionRequest) -> String {
        PATH.to_string()
    }

    fn decode_start(&self, output: &TextOutput) -> Result<Started<Token>, AiError> {
        decode_start(output)
    }

    fn status_path(&self, token: &Token) -> String {
        format!("{PATH}/{}", token.transcript_id)
    }

    fn decode_status(
        &self,
        output: &TextOutput,
        context: &PollContext<Token>,
    ) -> Result<Snapshot, AiError> {
        decode_status(output, context)
    }

    fn result_path(&self, token: &Token) -> String {
        format!("{PATH}/{}", token.transcript_id)
    }

    fn decode_result(
        &self,
        output: &TextOutput,
        context: &PollContext<Token>,
    ) -> Result<TranscriptionResponse, AiError> {
        decode_result(output, context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::model::TranscriptionTimestamps;
    use serde_json::json;

    fn request() -> TranscriptionRequest {
        TranscriptionRequest {
            model: "universal".into(),
            audio: media::url("https://x/a.mp3", Some("audio/mpeg".to_string()), None),
            language: None,
            prompt: None,
            timestamps: None,
            diarize: None,
            speakers: None,
            provider_options: None,
            http: None,
        }
    }

    #[test]
    fn url_audio_lowers_audio_url_and_language_detection() {
        let Body::Json { value, .. } = body_from(&request()).unwrap() else { panic!() };
        assert_eq!(value["audio_url"], json!("https://x/a.mp3"));
        assert_eq!(value["speech_models"], json!(["universal"]));
        assert_eq!(value["language_detection"], json!(true));

        let mut english = request();
        english.language = Some("en".into());
        let Body::Json { value, .. } = body_from(&english).unwrap() else { panic!() };
        assert_eq!(value["language_code"], json!("en"));
        assert!(value.get("language_detection").is_none());
    }

    #[test]
    fn segment_timestamps_or_speakers_enable_labels() {
        let mut segments = request();
        segments.timestamps = Some(TranscriptionTimestamps::Segment);
        let Body::Json { value, .. } = body_from(&segments).unwrap() else { panic!() };
        assert_eq!(value["speaker_labels"], json!(true));

        let mut speakers = request();
        speakers.speakers = Some(2);
        let Body::Json { value, .. } = body_from(&speakers).unwrap() else { panic!() };
        assert_eq!(value["speaker_labels"], json!(true));
        assert_eq!(value["speakers_expected"], json!(2));
    }

    #[test]
    fn prepare_swaps_inline_audio_for_the_upload_url() {
        let mut inline = request();
        inline.audio = media::base64("AQID", "audio/mpeg");
        let prepared = prepare(&inline, "https://cdn/a.mp3").unwrap();
        assert_eq!(prepared.audio.url(), Some("https://cdn/a.mp3"));
        assert_eq!(prepared.audio.media_type, "audio/mpeg");
        // URL audio passes through untouched.
        let passthrough = prepare(&request(), "ignored").unwrap();
        assert_eq!(passthrough.audio.url(), Some("https://x/a.mp3"));
    }

    #[test]
    fn upload_body_is_octet_stream_binary() {
        let mut inline = request();
        inline.audio = media::base64("AQID", "audio/mpeg");
        let Body::Binary { value, content_type, .. } = upload_body(&inline).unwrap() else {
            panic!()
        };
        assert_eq!(value, vec![1, 2, 3]);
        assert_eq!(content_type, "application/octet-stream");
    }

    #[test]
    fn decode_result_milliseconds_become_seconds() {
        let identity = identity();
        let auth = BTreeMap::new();
        let token = Token { transcript_id: "tr1".into() };
        let context = PollContext { token: &token, auth: &auth };
        let output = TextOutput {
            identity: &identity,
            body: r#"{"id":"tr1","status":"completed","text":"hi","words":[{"text":"hi","start":120,"end":640,"confidence":0.9,"speaker":"A"}],"utterances":[{"text":"hi","start":120,"end":640,"speaker":"A"}],"language_code":"EN","audio_duration":1.5,"speech_model_used":"universal"}"#,
            http: None,
        };
        let response = decode_result(&output, &context).unwrap();
        assert_eq!(response.text, "hi");
        assert_eq!(response.words.as_ref().unwrap()[0].start_seconds, 0.12);
        assert_eq!(response.words.as_ref().unwrap()[0].end_seconds, 0.64);
        assert_eq!(response.segments.as_ref().unwrap()[0].speaker.as_deref(), Some("A"));
        assert_eq!(response.language.as_deref(), Some("en"));
        assert_eq!(response.usage, Some(MediaUsage::Seconds { seconds: 1.5 }));
        assert_eq!(
            response.provider_metadata.as_ref().unwrap()["assemblyai"]["speechModel"],
            json!("universal")
        );

        let failed = TextOutput {
            identity: &identity,
            body: r#"{"id":"tr1","status":"error","error":"could not download"}"#,
            http: None,
        };
        let error = decode_result(&failed, &context).unwrap_err();
        assert!(error.message().contains("could not download"));

        let processing = TextOutput {
            identity: &identity,
            body: r#"{"id":"tr1","status":"processing"}"#,
            http: None,
        };
        let error = decode_result(&processing, &context).unwrap_err();
        assert!(error.message().contains("has not finished"));
    }
}
