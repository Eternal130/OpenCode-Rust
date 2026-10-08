//! text/event-stream subscription: parse `data:` frames and heartbeat
//! comments off `GET /api/event`, collect N data frames with a timeout, and
//! normalize the collected sequence.

use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde_json::Value;

use crate::minih::{self, BodyStream};
use crate::normalize::{normalize_value, NormEnv};

/// One parsed SSE frame. Comment-only frames (heartbeats) have empty `data`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// `event:` field, when present.
    pub event: Option<String>,
    /// Joined `data:` lines (`\n` between multiples).
    pub data: String,
    /// `:` comment lines seen since the previous dispatch.
    pub comments: Vec<String>,
}

impl Frame {
    /// Whether this frame carries a payload (as opposed to a heartbeat).
    #[must_use]
    pub fn is_data(&self) -> bool {
        !self.data.is_empty()
    }
}

/// Incremental SSE line parser (one `feed_line` per line; blank line
/// dispatches). Kept separate from IO so it is unit-testable.
#[derive(Default)]
pub(crate) struct Parser {
    event: Option<String>,
    data_lines: Vec<String>,
    comments: Vec<String>,
}

impl Parser {
    /// Feed one line (without its terminator). Returns a frame when a blank
    /// line dispatches one.
    pub(crate) fn feed_line(&mut self, line: &str) -> Option<Frame> {
        if line.is_empty() {
            let frame = Frame {
                event: self.event.take(),
                data: std::mem::take(&mut self.data_lines).join("\n"),
                comments: std::mem::take(&mut self.comments),
            };
            return Some(frame);
        }
        if let Some(comment) = line.strip_prefix(':') {
            self.comments.push(comment.trim().to_string());
            return None;
        }
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        // SSE strips a single leading space after the colon.
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "event" => self.event = Some(value.to_string()),
            "data" => self.data_lines.push(value.to_string()),
            _ => {} // id:/retry:/unknown fields are not needed for parity
        }
        None
    }
}

/// A live subscription to an SSE endpoint.
pub struct SseStream {
    body: BodyStream,
    parser: Parser,
    pending: Vec<u8>,
    eof: bool,
}

impl SseStream {
    /// Subscribe to `path` (normally `/api/event`) with Basic auth.
    /// Fails on non-200 or non-event-stream responses.
    pub async fn subscribe(
        base: &str,
        password: &str,
        path: &str,
        timeout: Duration,
    ) -> Result<Self> {
        let auth = minih::basic_auth("opencode", password);
        let (status, headers, body) =
            minih::stream_request(base, Some(&auth), "text/event-stream", path, timeout).await?;
        if status != 200 {
            return Err(anyhow!("SSE subscribe to {path} returned HTTP {status}"));
        }
        let content_type = headers
            .iter()
            .find(|(name, _)| name == "content-type")
            .map(|(_, value)| value.clone())
            .unwrap_or_default();
        if !content_type.starts_with("text/event-stream") {
            return Err(anyhow!("SSE subscribe to {path} returned content-type {content_type}"));
        }
        Ok(Self { body, parser: Parser::default(), pending: Vec::new(), eof: false })
    }

    /// Pull one complete line from the wire (or the flush remainder on EOF).
    async fn next_line(&mut self, timeout: Duration) -> Result<Option<String>> {
        loop {
            if let Some(pos) = self.pending.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = self.pending.drain(..=pos).collect();
                let mut text = String::from_utf8_lossy(&line).into_owned();
                text.pop(); // '\n'
                if text.ends_with('\r') {
                    text.pop();
                }
                return Ok(Some(text));
            }
            if self.eof {
                if self.pending.is_empty() {
                    return Ok(None);
                }
                let text = String::from_utf8_lossy(&self.pending).into_owned();
                self.pending.clear();
                return Ok(Some(text));
            }
            match self.body.next_bytes(timeout).await? {
                Some(bytes) => self.pending.extend_from_slice(&bytes),
                None => self.eof = true,
            }
        }
    }

    /// Await the next frame. `idle_timeout` bounds the wait for any single
    /// wire read, so a silent server surfaces as an error instead of a hang.
    pub async fn next_frame(&mut self, idle_timeout: Duration) -> Result<Frame> {
        loop {
            let dispatched = match self.next_line(idle_timeout).await? {
                Some(line) => self.parser.feed_line(&line),
                None => {
                    // Flush a trailing partial frame, then fail: the stream
                    // ended with nothing more to dispatch.
                    self.parser.feed_line("")
                }
            };
            if let Some(frame) = dispatched {
                return Ok(frame);
            }
            if self.eof && self.pending.is_empty() {
                return Err(anyhow!("SSE stream ended"));
            }
        }
    }

    /// Collect `count` data frames (heartbeats are skipped) within
    /// `overall_timeout`.
    pub async fn collect(&mut self, count: usize, overall_timeout: Duration) -> Result<Vec<Frame>> {
        let deadline = tokio::time::Instant::now() + overall_timeout;
        let mut frames = Vec::with_capacity(count);
        while frames.len() < count {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(anyhow!(
                    "timed out after {overall_timeout:?} with {}/{} SSE frames",
                    frames.len(),
                    count
                ));
            }
            let frame = self.next_frame(remaining).await.context("collecting SSE frames")?;
            if frame.is_data() {
                frames.push(frame);
            }
        }
        Ok(frames)
    }
}

/// Render collected frames as a normalized JSON array preserving sequence:
/// each entry records the dispatch index, optional `event`, optional
/// `comments`, and the payload (parsed JSON normalized through
/// [`normalize_value`]; non-JSON payloads stay strings).
#[must_use]
pub fn frames_to_json(frames: &[Frame], env: &NormEnv) -> Value {
    Value::Array(
        frames
            .iter()
            .enumerate()
            .map(|(index, frame)| {
                let mut entry = serde_json::Map::new();
                entry.insert("index".to_string(), Value::from(index));
                if let Some(event) = &frame.event {
                    entry.insert("event".to_string(), Value::String(event.clone()));
                }
                if !frame.comments.is_empty() {
                    entry.insert(
                        "comments".to_string(),
                        Value::Array(
                            frame
                                .comments
                                .iter()
                                .map(|comment| Value::String(comment.clone()))
                                .collect(),
                        ),
                    );
                }
                let data = serde_json::from_str::<Value>(&frame.data).map_or_else(
                    |_| Value::String(frame.data.clone()),
                    |parsed| normalize_value(&parsed, env),
                );
                entry.insert("data".to_string(), data);
                Value::Object(entry)
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_data_event_and_heartbeat_frames() {
        let mut parser = Parser::default();
        assert!(parser
            .feed_line(
                "data: {\"id\":\"evt_11c556f9e001H4T7m94mksITaJ\",\"type\":\"server.connected\"}"
            )
            .is_none());
        assert!(parser.feed_line("").is_some());
        let heartbeat = parser.feed_line(": heartbeat").is_none();
        assert!(heartbeat);
        let frame = parser.feed_line("").unwrap();
        assert!(!frame.is_data());
        assert_eq!(frame.comments, vec!["heartbeat".to_string()]);
    }

    #[test]
    fn joins_multi_line_data_and_strips_single_space() {
        let mut parser = Parser::default();
        parser.feed_line("event: session.updated");
        parser.feed_line("data:line1");
        parser.feed_line("data:  line2");
        let frame = parser.feed_line("").unwrap();
        assert_eq!(frame.event.as_deref(), Some("session.updated"));
        // Exactly one leading space is stripped after the colon (SSE spec).
        assert_eq!(frame.data, "line1\n line2");
    }

    #[test]
    fn normalizes_frame_sequence_to_json() {
        let frames = vec![
            Frame {
                event: None,
                data: "{\"id\":\"evt_11c556f9e001H4T7m94mksITaJ\",\"type\":\"server.connected\",\"data\":{}}".to_string(),
                comments: vec![],
            },
            Frame { event: None, data: "plain text".to_string(), comments: vec!["heartbeat".to_string()] },
        ];
        let out = frames_to_json(&frames, &NormEnv::new());
        assert_eq!(out[0]["data"]["id"], Value::from("<EVT_ID>"));
        assert_eq!(out[0]["data"]["type"], Value::from("server.connected"));
        assert_eq!(out[1]["data"], Value::from("plain text"));
        assert_eq!(out[1]["comments"][0], Value::from("heartbeat"));
    }
}
