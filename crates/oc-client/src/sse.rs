//! SSE decoding per server-deep-map §7 (v2 client `sse` iterator parity):
//! content-type gate lives in [`crate::transport`]; here: 16 MiB buffer cap,
//! CRLF normalization, `\n\n` framing, ALL `data:` lines joined with `\n`,
//! heartbeats skipped, JSON parse failure -> `MalformedResponse`,
//! `onActivity` per received chunk.

use futures::StreamExt;

use crate::error::ClientError;

/// Hard cap on buffered (undelivered) SSE bytes (v2 `maxSseEventBytes`).
pub const MAX_SSE_EVENT_BYTES: usize = 16 * 1024 * 1024;

/// One decoded SSE frame. Blocks that carry no `data:` lines (heartbeats such
/// as `: heartbeat`) never surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseFrame {
    pub id: Option<String>,
    pub event: Option<String>,
    pub data: String,
}

/// Byte-stream SSE decoder over a reqwest response body.
pub struct SseStream {
    inner: std::pin::Pin<Box<dyn futures::Stream<Item = reqwest::Result<bytes::Bytes>> + Send>>,
    buf: Vec<u8>,
    done: bool,
    cap: usize,
    double_decode: bool,
    on_activity: Option<Box<dyn FnMut() + Send>>,
}

impl SseStream {
    pub(crate) fn new(response: reqwest::Response, double_decode: bool) -> Self {
        Self {
            inner: Box::pin(response.bytes_stream()),
            buf: Vec::new(),
            done: false,
            cap: MAX_SSE_EVENT_BYTES,
            double_decode,
            on_activity: None,
        }
    }

    /// Overrides the 16 MiB buffer cap (tests).
    #[must_use]
    pub fn with_max_buffer(mut self, cap: usize) -> Self {
        self.cap = cap;
        self
    }

    /// `onActivity`: invoked for every received chunk, including keepalive
    /// comments that yield no frame.
    #[must_use]
    pub fn with_on_activity(mut self, callback: impl FnMut() + Send + 'static) -> Self {
        self.on_activity = Some(Box::new(callback));
        self
    }

    /// Whether `data` payloads are JSON-in-string (`V2EventEncoded` et al.) and
    /// get double-decoded by [`SseStream::next_json`].
    #[must_use]
    pub fn double_decode(&self) -> bool {
        self.double_decode
    }

    /// Next data-carrying frame; `None` on stream end. Heartbeat/comment-only
    /// blocks are skipped.
    pub async fn next_frame(&mut self) -> Result<Option<SseFrame>, ClientError> {
        loop {
            if let Some(boundary) = find_nl_nl(&self.buf) {
                let mut block: Vec<u8> = self.buf.drain(..boundary + 2).collect();
                block.truncate(boundary);
                if let Some(frame) = parse_block(&block) {
                    return Ok(Some(frame));
                }
                continue;
            }
            if self.done {
                if self.buf.is_empty() {
                    return Ok(None);
                }
                // Flush a trailing unterminated block (v2 appends "\n\n").
                self.buf.extend_from_slice(b"\n\n");
                continue;
            }
            let chunk = match self.inner.next().await {
                Some(Ok(chunk)) => chunk,
                Some(Err(e)) => return Err(ClientError::from(e)),
                None => {
                    self.done = true;
                    continue;
                }
            };
            if !chunk.is_empty() {
                if let Some(callback) = &mut self.on_activity {
                    callback();
                }
                self.buf.extend_from_slice(&chunk);
                if self.buf.len() > self.cap {
                    return Err(ClientError::SseEventTooLarge(self.cap));
                }
                normalize_crlf(&mut self.buf);
            }
        }
    }

    /// Next frame parsed as JSON. With `double_decode` (JSON-in-string event
    /// payloads such as `V2EventEncoded`) the outer JSON string is parsed
    /// again.
    pub async fn next_json(&mut self) -> Result<Option<serde_json::Value>, ClientError> {
        let Some(frame) = self.next_frame().await? else {
            return Ok(None);
        };
        let value: serde_json::Value = serde_json::from_str(&frame.data)
            .map_err(|e| ClientError::MalformedResponse(format!("sse data: {e}")))?;
        if self.double_decode {
            if let Some(inner) = value.as_str() {
                let decoded: serde_json::Value = serde_json::from_str(inner)
                    .map_err(|e| ClientError::MalformedResponse(format!("sse inner json: {e}")))?;
                return Ok(Some(decoded));
            }
        }
        Ok(Some(value))
    }

    /// Convenience [`futures::Stream`] over [`SseStream::next_json`].
    pub fn into_json_stream(
        self,
    ) -> impl futures::Stream<Item = Result<serde_json::Value, ClientError>> {
        futures::stream::unfold(self, |mut stream| async move {
            match stream.next_json().await {
                Ok(Some(value)) => Some((Ok(value), stream)),
                Ok(None) => None,
                Err(e) => Some((Err(e), stream)),
            }
        })
    }
}

fn find_nl_nl(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\n\n")
}

/// `\r\n` and lone `\r` both normalize to `\n` (safe: multi-byte UTF-8 never
/// contains ASCII bytes).
fn normalize_crlf(buf: &mut Vec<u8>) {
    let mut out = Vec::with_capacity(buf.len());
    let mut i = 0;
    while i < buf.len() {
        if buf[i] == b'\r' {
            if buf.get(i + 1) == Some(&b'\n') {
                i += 1;
            }
            out.push(b'\n');
        } else {
            out.push(buf[i]);
        }
        i += 1;
    }
    *buf = out;
}

/// v2 parity: `data:` lines are `slice(5).trimStart()`-ed and joined with `\n`;
/// comment lines (leading `:`) are ignored; frames without data are skipped.
fn parse_block(block: &[u8]) -> Option<SseFrame> {
    let text = String::from_utf8_lossy(block);
    let mut data: Vec<String> = Vec::new();
    let mut id = None;
    let mut event = None;
    for line in text.split('\n') {
        if let Some(value) = line.strip_prefix("data:") {
            data.push(value.trim_start().to_owned());
        } else if let Some(value) = line.strip_prefix("id:") {
            id = Some(value.trim_start().to_owned());
        } else if let Some(value) = line.strip_prefix("event:") {
            event = Some(value.trim_start().to_owned());
        }
        // comments and unknown fields are ignored (heartbeat = comment-only)
    }
    if data.is_empty() {
        return None;
    }
    Some(SseFrame { id, event, data: data.join("\n") })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames(input: &[u8]) -> Vec<SseFrame> {
        // Feeding the buffer directly mirrors the state after chunk arrival
        // (which always CRLF-normalizes).
        let mut normalized = input.to_vec();
        normalize_crlf(&mut normalized);
        let mut stream = SseStream {
            inner: Box::pin(futures::stream::empty()),
            buf: normalized,
            done: true,
            cap: MAX_SSE_EVENT_BYTES,
            double_decode: false,
            on_activity: None,
        };
        let mut out = Vec::new();
        while let Ok(Some(frame)) = futures::executor::block_on(stream.next_frame()) {
            out.push(frame);
        }
        out
    }

    #[test]
    fn joins_multi_line_data() {
        let parsed = frames(b"data: {\"a\":\ndata: 1}\n\n");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].data, "{\"a\":\n1}");
    }

    #[test]
    fn skips_heartbeats_and_comments() {
        let parsed = frames(b": heartbeat\n\ndata: 1\n\n: keepalive\ndata: 2\n\n");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[1].data, "2");
    }

    #[test]
    fn crlf_normalization() {
        let parsed = frames(b"data: x\r\n\r\ndata: y\r\r");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].data, "x");
        assert_eq!(parsed[1].data, "y");
    }

    #[test]
    fn captures_id_and_event() {
        let parsed = frames(b"id: 7\nevent: message.updated\ndata: {}\n\n");
        assert_eq!(parsed[0].id.as_deref(), Some("7"));
        assert_eq!(parsed[0].event.as_deref(), Some("message.updated"));
    }
}
