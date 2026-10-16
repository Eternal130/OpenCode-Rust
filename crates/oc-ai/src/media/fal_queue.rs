//! `fal-queue` shared queued-protocol template (port of
//! `protocols/utils/fal-queue.ts`). fal hands back absolute follow-up URLs
//! on submit; they are authoritative for status, result, and cancel.

use std::collections::BTreeMap;

use crate::media::model::{PollContext, Snapshot, Started, Status};
use crate::media::protocol::{status as table_status, Identity, TextOutput};
use crate::schema::AiError;

pub const DEFAULT_BASE_URL: &str = "https://queue.fal.run";

/// The serializable fal queue token (fal-queue.ts:12-18).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FalToken {
    pub request_id: String,
    pub status_url: String,
    pub response_url: String,
    pub cancel_url: String,
}

/// `FalQueue.mediaUrl` (fal-queue.ts:41-42): fal accepts public URLs and
/// data URIs; there is no provider file handle to forward.
pub fn media_url(asset: &crate::schema::media::Asset, name: &str) -> Result<String, AiError> {
    crate::media::utils::media_reference(asset, None, name).map(|(_, value)| value)
}

fn status_table() -> BTreeMap<&'static str, Status> {
    [
        ("IN_QUEUE", Status::Queued),
        ("IN_PROGRESS", Status::Running),
        ("COMPLETED", Status::Completed),
    ]
    .into_iter()
    .collect()
}

/// Decode a fal queue submission into the token + first snapshot
/// (fal-queue.ts:56-70).
pub fn decode_start(output: &TextOutput) -> Result<Started<FalToken>, AiError> {
    let value = output.decode_json(output.body)?;
    let position = value.get("queue_position").and_then(|v| v.as_f64());
    let request_id =
        value.get("request_id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
    Ok(Started {
        snapshot: Snapshot {
            id: request_id.clone(),
            status: Status::Queued,
            progress: None,
            position,
        },
        token: FalToken {
            request_id,
            status_url: value
                .get("status_url")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            response_url: value
                .get("response_url")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            cancel_url: value
                .get("cancel_url")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
        },
    })
}

/// `decodeStatus` (fal-queue.ts:72-85): fal reports request failures as
/// COMPLETED with an `error`; the response endpoint carries the details.
pub fn decode_status(
    output: &TextOutput,
    context: &PollContext<FalToken>,
) -> Result<Snapshot, AiError> {
    let value = output.decode_json(output.body)?;
    let raw = value.get("status").and_then(|v| v.as_str()).unwrap_or_default();
    let status = table_status(&status_table(), raw, output)?;
    let failed = status == Status::Completed && value.get("error").is_some_and(|e| !e.is_null());
    Ok(Snapshot {
        id: context.token.request_id.clone(),
        status: if failed { Status::Failed } else { status },
        progress: None,
        position: (status == Status::Queued)
            .then(|| value.get("queue_position").and_then(|v| v.as_f64()))
            .flatten(),
    })
}

/// The cancel spec: `PUT` the absolute cancel URL (fal-queue.ts:87).
#[must_use]
pub fn cancel_spec() -> crate::media::protocol::CancelSpec {
    crate::media::protocol::CancelSpec { method: "PUT", active_only: false }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_start_reads_queue_position() {
        let identity = Identity::new("fal-images", "fal Images", "fal");
        let body = r#"{"request_id":"r1","status_url":"https://q.fal.run/s","response_url":"https://q.fal.run/r","cancel_url":"https://q.fal.run/c","queue_position":3}"#;
        let output = TextOutput { identity: &identity, body, http: None };
        let started = decode_start(&output).unwrap();
        assert_eq!(started.token.request_id, "r1");
        assert_eq!(started.snapshot.position, Some(3.0));
        assert_eq!(started.snapshot.status, Status::Queued);
    }

    #[test]
    fn completed_with_error_maps_to_failed() {
        let identity = Identity::new("fal-images", "fal Images", "fal");
        let body = r#"{"status":"COMPLETED","error":{"message":"boom"}}"#;
        let output = TextOutput { identity: &identity, body, http: None };
        let token = FalToken {
            request_id: "r1".into(),
            status_url: String::new(),
            response_url: String::new(),
            cancel_url: String::new(),
        };
        let auth = BTreeMap::new();
        let context = PollContext { token: &token, auth: &auth };
        let snapshot = decode_status(&output, &context).unwrap();
        assert_eq!(snapshot.status, Status::Failed);
    }

    #[test]
    fn unknown_status_is_invalid_output() {
        let identity = Identity::new("fal-images", "fal Images", "fal");
        let body = r#"{"status":"WAT"}"#;
        let output = TextOutput { identity: &identity, body, http: None };
        let token = FalToken {
            request_id: "r1".into(),
            status_url: String::new(),
            response_url: String::new(),
            cancel_url: String::new(),
        };
        let auth = BTreeMap::new();
        let context = PollContext { token: &token, auth: &auth };
        let error = decode_status(&output, &context).unwrap_err();
        assert!(error.message().contains("Unknown generation status"));
    }
}
