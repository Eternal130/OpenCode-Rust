//! AWS event-stream binary framing (port of `bedrock-event-stream.ts` over
//! `@smithy/eventstream-codec`): each frame is
//! `[total-length:4][headers-length:4][prelude-crc:4][headers][payload][message-crc:4]`
//! (big-endian, CRC32/IEEE). Decoded frames are re-wrapped by their
//! `:event-type` header as `{"<event-type>": <payload>}` JSON strings — or
//! `{"exception": {"type", "details"}}` for `:message-type: "exception"` —
//! so the Converse chunk schema matches the JSON payload directly.

use crate::schema::{AiError, AiErrorReason, ReasonFields};

/// One decoded event-stream frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    /// `:message-type` value (`event` | `exception` | `error`).
    pub message_type: String,
    /// `:event-type` (events) or `:exception-type` (exceptions).
    pub event_type: String,
    /// Decoded typed header values, keyed by name (only `:error-code` /
    /// `:error-message` are consumed today).
    pub headers: Vec<(String, HeaderValue)>,
    /// Raw payload bytes (may be empty).
    pub payload: Vec<u8>,
}

/// Typed event-stream header values (the value-type byte decides the width).
#[derive(Debug, Clone, PartialEq)]
pub enum HeaderValue {
    /// Type 7 — `u16` length + bytes.
    String(String),
    /// Type 4 — signed `i32` (also covers types 2/3 by width; unused today).
    Int(i64),
}

/// CRC32/IEEE (the reflected polynomial `0xEDB88320` the AWS event-stream
/// codec validates prelude + message CRCs with).
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// Decode error classification: CRC/frame failures are invalid provider
/// output (the stream is corrupt); `error` message-frames carry their own
/// code/message.
fn invalid(message: String) -> AiError {
    AiError::new(AiErrorReason::InvalidProviderOutput {
        fields: ReasonFields::new(message),
        classification: None,
        route: Some("bedrock-converse".into()),
    })
}

impl Frame {
    pub fn header(&self, name: &str) -> Option<&HeaderValue> {
        self.headers.iter().find(|(key, _)| key == name).map(|(_, value)| value)
    }
}

/// Decode one complete frame from `bytes` (exactly `total_length` long).
/// Returns the frame; CRC or structure failures are typed AI errors.
pub fn decode_frame(bytes: &[u8]) -> Result<Frame, AiError> {
    let fail = |message: &str| {
        invalid(format!("Failed to decode Bedrock Converse event-stream frame: {message}"))
    };
    if bytes.len() < 12 {
        return Err(fail("frame shorter than the 12-byte prelude"));
    }
    let total_length = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    let headers_length = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
    if total_length != bytes.len() {
        return Err(fail("frame length does not match the buffered bytes"));
    }
    if headers_length + 12 > total_length {
        return Err(fail("headers length exceeds the frame"));
    }
    let prelude_crc = u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
    if crc32(&bytes[..8]) != prelude_crc {
        return Err(fail("prelude CRC mismatch"));
    }
    let message_crc = u32::from_be_bytes([
        bytes[total_length - 4],
        bytes[total_length - 3],
        bytes[total_length - 2],
        bytes[total_length - 1],
    ]);
    if crc32(&bytes[..total_length - 4]) != message_crc {
        return Err(fail("message CRC mismatch"));
    }
    let headers_bytes = &bytes[12..12 + headers_length];
    let mut cursor = 0usize;
    let mut headers = Vec::new();
    while cursor < headers_bytes.len() {
        let name_length = headers_bytes[cursor] as usize;
        cursor += 1;
        if cursor + name_length + 1 > headers_bytes.len() {
            return Err(fail("truncated header name"));
        }
        let name =
            String::from_utf8_lossy(&headers_bytes[cursor..cursor + name_length]).into_owned();
        cursor += name_length;
        let value_type = headers_bytes[cursor];
        cursor += 1;
        let value = match value_type {
            7 => {
                if cursor + 2 > headers_bytes.len() {
                    return Err(fail("truncated string header"));
                }
                let length =
                    u16::from_be_bytes([headers_bytes[cursor], headers_bytes[cursor + 1]]) as usize;
                cursor += 2;
                if cursor + length > headers_bytes.len() {
                    return Err(fail("truncated string header value"));
                }
                let value =
                    String::from_utf8_lossy(&headers_bytes[cursor..cursor + length]).into_owned();
                cursor += length;
                HeaderValue::String(value)
            }
            2..=5 => {
                let width = match value_type {
                    2 => 1,
                    3 => 2,
                    4 => 4,
                    _ => 8,
                };
                if cursor + width > headers_bytes.len() {
                    return Err(fail("truncated integer header"));
                }
                let mut buffer = [0u8; 8];
                buffer[8 - width..].copy_from_slice(&headers_bytes[cursor..cursor + width]);
                cursor += width;
                HeaderValue::Int(i64::from_be_bytes(buffer))
            }
            _ => return Err(fail(&format!("unsupported header value type {value_type}"))),
        };
        headers.push((name, value));
    }
    let payload = bytes[12 + headers_length..total_length - 4].to_vec();
    let message_type = headers
        .iter()
        .find(|(name, _)| name == ":message-type")
        .and_then(|(_, value)| match value {
            HeaderValue::String(value) => Some(value.clone()),
            _ => None,
        })
        .unwrap_or_else(|| "event".to_string());
    let event_type = headers
        .iter()
        .find(|(name, _)| name == ":event-type" || name == ":exception-type")
        .and_then(|(_, value)| match value {
            HeaderValue::String(value) => Some(value.clone()),
            _ => None,
        })
        .unwrap_or_default();
    Ok(Frame { message_type, event_type, headers, payload })
}

/// The framing layer's re-wrap step (port of `consumeFrames`'s output
/// shape): parse the payload JSON, drop the `p` padding field AWS pads
/// short payloads with, and wrap under the event name. `None` for frames
/// without a usable event type or payload.
pub fn frame_to_json(frame: &Frame) -> Result<Option<String>, AiError> {
    if frame.message_type == "error" {
        let code = frame
            .header(":error-code")
            .and_then(|value| match value {
                HeaderValue::String(value) => Some(value.clone()),
                _ => None,
            })
            .unwrap_or_default();
        let message = frame
            .header(":error-message")
            .and_then(|value| match value {
                HeaderValue::String(value) => Some(value.clone()),
                _ => None,
            })
            .unwrap_or_default();
        let text = [code, message]
            .iter()
            .filter(|part| !part.is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join(": ");
        let text =
            if text.is_empty() { "Bedrock Converse event-stream error".to_string() } else { text };
        return Err(AiError::event_error("bedrock-converse", text));
    }
    if frame.event_type.is_empty() || frame.payload.is_empty() {
        return Ok(None);
    }
    let payload = std::str::from_utf8(&frame.payload).map_err(|error| {
        invalid(format!("Failed to parse Bedrock Converse event-stream payload: {error}"))
    })?;
    let mut parsed: serde_json::Value = serde_json::from_str(payload).map_err(|error| {
        invalid(format!("Failed to parse Bedrock Converse event-stream payload: {error}"))
    })?;
    if let serde_json::Value::Object(object) = &mut parsed {
        object.remove("p");
    }
    let wrapped = if frame.message_type == "exception" {
        serde_json::json!({ "exception": { "type": frame.event_type, "details": parsed } })
    } else {
        serde_json::json!({ frame.event_type.clone(): parsed })
    };
    Ok(Some(wrapped.to_string()))
}

// =============================================================================
// Encoder (tests + fixtures): builds spec-shaped frames with CRCs.
// =============================================================================

#[cfg(test)]
pub(crate) mod encode {
    use super::{crc32, HeaderValue};

    fn put_u32(out: &mut Vec<u8>, value: u32) {
        out.extend_from_slice(&value.to_be_bytes());
    }

    fn header(out: &mut Vec<u8>, name: &str, value: &HeaderValue) {
        out.push(name.len() as u8);
        out.extend_from_slice(name.as_bytes());
        match value {
            HeaderValue::String(value) => {
                out.push(7);
                put_u16(out, value.len() as u16);
                out.extend_from_slice(value.as_bytes());
            }
            HeaderValue::Int(value) => {
                out.push(4);
                out.extend_from_slice(&value.to_be_bytes());
            }
        }
    }

    fn put_u16(out: &mut Vec<u8>, value: u16) {
        out.extend_from_slice(&value.to_be_bytes());
    }

    /// Encode one event frame: `:message-type` default `event`.
    pub fn event(event_type: &str, payload: &str) -> Vec<u8> {
        frame("event", event_type, payload, Vec::<(String, HeaderValue)>::new())
    }

    pub fn frame(
        message_type: &str,
        event_type: &str,
        payload: &str,
        extra_headers: Vec<(String, HeaderValue)>,
    ) -> Vec<u8> {
        let mut headers = vec![
            (":message-type".to_string(), HeaderValue::String(message_type.to_string())),
            (
                if message_type == "exception" { ":exception-type" } else { ":event-type" }
                    .to_string(),
                HeaderValue::String(event_type.to_string()),
            ),
        ];
        headers.extend(extra_headers);
        let mut headers_bytes = Vec::new();
        for (name, value) in &headers {
            header(&mut headers_bytes, name, value);
        }
        let payload_bytes = payload.as_bytes();
        let total_length = 12 + headers_bytes.len() + payload_bytes.len() + 4;
        let mut out = Vec::with_capacity(total_length);
        put_u32(&mut out, total_length as u32);
        put_u32(&mut out, headers_bytes.len() as u32);
        let prelude_crc = crc32(&out);
        put_u32(&mut out, prelude_crc);
        out.extend_from_slice(&headers_bytes);
        out.extend_from_slice(payload_bytes);
        let message_crc = crc32(&out);
        put_u32(&mut out, message_crc);
        debug_assert_eq!(out.len(), total_length);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CRC32/IEEE check value (catalogued vector).
    #[test]
    fn crc32_matches_the_catalogued_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn frames_round_trip_with_event_type_and_payload() {
        let bytes =
            encode::event("contentBlockDelta", r#"{"contentBlockIndex":0,"delta":{"text":"hi"}}"#);
        let frame = decode_frame(&bytes).expect("frame decodes");
        assert_eq!(frame.message_type, "event");
        assert_eq!(frame.event_type, "contentBlockDelta");
        let wrapped = frame_to_json(&frame).unwrap().expect("wrapped");
        assert_eq!(
            wrapped,
            r#"{"contentBlockDelta":{"contentBlockIndex":0,"delta":{"text":"hi"}}}"#
        );
    }

    #[test]
    fn short_payloads_drop_the_padding_field() {
        let bytes = encode::event("messageStart", r#"{"role":"assistant","p":""}"#);
        let frame = decode_frame(&bytes).unwrap();
        assert_eq!(
            frame_to_json(&frame).unwrap().unwrap(),
            r#"{"messageStart":{"role":"assistant"}}"#
        );
    }

    #[test]
    fn exception_frames_wrap_type_and_details() {
        let bytes = encode::frame(
            "exception",
            "ValidationException",
            r#"{"message":"field is required"}"#,
            vec![],
        );
        let frame = decode_frame(&bytes).unwrap();
        assert_eq!(frame.message_type, "exception");
        let wrapped = frame_to_json(&frame).unwrap().unwrap();
        let value: serde_json::Value = serde_json::from_str(&wrapped).unwrap();
        assert_eq!(value["exception"]["type"], "ValidationException");
        assert_eq!(value["exception"]["details"]["message"], "field is required");
    }

    #[test]
    fn error_message_frames_surface_code_and_message() {
        let bytes = encode::frame(
            "error",
            "",
            "",
            vec![
                (":error-code".to_string(), HeaderValue::String("throttled".into())),
                (":error-message".to_string(), HeaderValue::String("too fast".into())),
            ],
        );
        let frame = decode_frame(&bytes).unwrap();
        let error = frame_to_json(&frame).unwrap_err();
        assert_eq!(error.message(), "throttled: too fast");
    }

    #[test]
    fn corrupt_crcs_are_invalid_provider_output() {
        let mut bytes = encode::event("messageStop", r#"{"stopReason":"end_turn"}"#);
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        let error = decode_frame(&bytes).unwrap_err();
        assert!(
            matches!(error.reason, AiErrorReason::InvalidProviderOutput { .. }),
            "{}",
            error.tag()
        );
    }

    #[test]
    fn frames_without_payloads_or_types_produce_no_json() {
        let bytes = encode::event("messageStart", "");
        let frame = decode_frame(&bytes).unwrap();
        assert!(frame_to_json(&frame).unwrap().is_none());
    }
}
