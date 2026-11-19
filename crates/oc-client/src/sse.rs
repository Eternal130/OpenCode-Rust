//! SSE decoding per server-deep-map §7 (v2 client `sse` iterator parity):
//! content-type gate lives in [`crate::transport`]; here: 16 MiB buffer cap,
//! CRLF normalization, `\n\n` framing, ALL `data:` lines joined with `\n`,
//! heartbeats skipped, JSON parse failure -> `MalformedResponse`,
//! `onActivity` per received chunk.

use futures::StreamExt;

use crate::error::ClientError;

/// Hard cap on buffered (undelivered) SSE content, counted in UTF-16
/// code units (G-126: v2 `maxSseEventBytes` compares against the decoded
/// JS string's `.length`, i.e. UTF-16 code units — NOT bytes, so CJK or
/// emoji payloads up to 3× larger in bytes pass). The constant name
/// keeps the ref's `maxSseEventBytes` spelling; the unit is chars.
pub const MAX_SSE_EVENT_CHARS: usize = 16 * 1024 * 1024;

/// Back-compat alias for the pre-G-126 name (same value; the limit is
/// counted in UTF-16 code units since G-126).
pub const MAX_SSE_EVENT_BYTES: usize = MAX_SSE_EVENT_CHARS;

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
            cap: MAX_SSE_EVENT_CHARS,
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

    /// [`next_json`] with a **byte-activity-aware** idle deadline
    /// (2026-11-19 connection-flap incident): the deadline fires only
    /// when NO bytes arrive for `idle` — heartbeat/comment blocks keep
    /// the stream alive exactly like ref's per-read idle on the raw
    /// stream (its connection reads every chunk; any bytes reset it).
    /// The plain `next_json` under a timeout is BLIND to comments
    /// (they parse to no frame), so a quiet bus tripped the TUI's 45s
    /// watchdog every cycle despite healthy 15s heartbeats.
    pub async fn next_json_alive(
        &mut self,
        idle: std::time::Duration,
    ) -> Result<Option<serde_json::Value>, ClientError> {
        let mut last_byte = std::time::Instant::now();
        loop {
            if let Some(boundary) = find_nl_nl(&self.buf) {
                let mut block: Vec<u8> = self.buf.drain(..boundary + 2).collect();
                block.truncate(boundary);
                if let Some(frame) = parse_block(&block) {
                    return self.frame_to_json(&frame);
                }
                continue; // comment/heartbeat block — ALIVE, keep parsing
            }
            if self.done {
                if self.buf.is_empty() {
                    return Ok(None);
                }
                self.buf.extend_from_slice(b"\n\n");
                continue;
            }
            let chunk = match tokio::time::timeout(
                idle.saturating_sub(last_byte.elapsed()),
                self.inner.next(),
            )
            .await
            {
                Ok(Some(Ok(chunk))) => chunk,
                Ok(Some(Err(e))) => return Err(ClientError::from(e)),
                Ok(None) => {
                    self.done = true;
                    continue;
                }
                Err(_) => {
                    return Err(ClientError::MalformedResponse(
                        "event stream stalled: no bytes within the idle deadline".to_owned(),
                    ));
                }
            };
            if !chunk.is_empty() {
                last_byte = std::time::Instant::now();
                if let Some(callback) = &mut self.on_activity {
                    callback();
                }
                self.buf.extend_from_slice(&chunk);
                if exceeds_char_limit(&self.buf, self.cap) {
                    return Err(ClientError::SseEventTooLarge(self.cap));
                }
                normalize_crlf(&mut self.buf);
            }
        }
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
                if exceeds_char_limit(&self.buf, self.cap) {
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
        self.frame_to_json(&frame)
    }

    /// Decode one parsed frame's `data` payload (shared by
    /// [`next_json`] and [`next_json_alive`]).
    fn frame_to_json(
        &mut self,
        frame: &SseFrame,
    ) -> Result<Option<serde_json::Value>, ClientError> {
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

/// Whether the buffered content exceeds `cap` UTF-16 code units (G-126:
/// the ref compares the decoded JS string's `.length` — UTF-16 code
/// units — against `maxSseEventBytes`). Exact, not approximated: a
/// UTF-16 code unit is at least one byte, so `buf.len() <= cap` always
/// passes and the (linear) `encode_utf16().count()` only runs on
/// buffers already over the cap in bytes — the hot path stays O(1).
/// Byte-counting proxies such as `chars().count()` would under-count
/// surrogate pairs (emoji) and are deliberately avoided.
fn exceeds_char_limit(buf: &[u8], cap: usize) -> bool {
    if buf.len() <= cap {
        return false;
    }
    // A chunk boundary may split a multi-byte char; lossy decoding
    // replaces the partial tail with one U+FFFD per invalid sequence,
    // which counts as one unit and is replaced by the real units on the
    // next chunk either way.
    String::from_utf8_lossy(buf).encode_utf16().count() > cap
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

    /// Drives the real chunk path (the char-limit check only runs on
    /// chunk arrival), one chunk per call.
    fn frames_chunked(chunks: Vec<Vec<u8>>, cap: usize) -> Result<Vec<SseFrame>, ClientError> {
        let mut stream = SseStream {
            inner: Box::pin(futures::stream::iter(
                chunks.into_iter().map(|c| Ok::<_, reqwest::Error>(bytes::Bytes::from(c))),
            )),
            buf: Vec::new(),
            done: false,
            cap,
            double_decode: false,
            on_activity: None,
        };
        let mut out = Vec::new();
        while let Some(frame) = futures::executor::block_on(stream.next_frame())? {
            out.push(frame);
        }
        Ok(out)
    }

    /// G-126: the 16 MiB cap counts UTF-16 code units, not bytes — a CJK
    /// payload over the cap in bytes but under it in units passes.
    /// (Units include the `data: ` framing: 6 + payload units.)
    #[test]
    fn char_limit_passes_cjk_bytes_over_the_cap() {
        // "你好".repeat(2) is 12 payload bytes but only 4 payload units;
        // with the framing the buffer is 20 bytes / 12 units — over a
        // 12-unit cap in BYTES, exactly at the cap in units.
        let data = format!("data: {}\n\n", "你好".repeat(2));
        assert_eq!(data.len(), 20);
        let parsed = frames_chunked(vec![data.into_bytes()], 12).expect("units at cap pass");
        assert_eq!(parsed.len(), 1);
    }

    /// G-126: over the cap in UTF-16 units the same error surface fires.
    #[test]
    fn char_limit_rejects_over_unit_cap() {
        let data = format!("data: {}\n\n", "ab".repeat(4)); // 14 units, 14 bytes
        let error = frames_chunked(vec![data.into_bytes()], 5).expect_err("units over cap fail");
        assert!(matches!(error, ClientError::SseEventTooLarge(5)), "{error}");
    }

    /// Emoji are surrogate pairs (2 UTF-16 units per character) — counted
    /// exactly, not via a `chars().count()` approximation.
    #[test]
    fn char_limit_counts_surrogate_pairs_as_two_units() {
        // 2 emoji = 4 units (+ 8 framing units = 12) fit a 12-unit cap;
        // a third (14 units) crosses it.
        let data = format!("data: {}\n\n", "😀".repeat(2));
        frames_chunked(vec![data.into_bytes()], 12).expect("12 units fit");
        let data = format!("data: {}\n\n", "😀".repeat(3));
        let error = frames_chunked(vec![data.into_bytes()], 12).expect_err("14 units over cap");
        assert!(matches!(error, ClientError::SseEventTooLarge(12)), "{error}");
    }

    fn frames(input: &[u8]) -> Vec<SseFrame> {
        // Feeding the buffer directly mirrors the state after chunk arrival
        // (which always CRLF-normalizes).
        let mut normalized = input.to_vec();
        normalize_crlf(&mut normalized);
        let mut stream = SseStream {
            inner: Box::pin(futures::stream::empty()),
            buf: normalized,
            done: true,
            cap: MAX_SSE_EVENT_CHARS,
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

    /// 2026-11-19 connection-flap regression, half 1: heartbeat comments
    /// are BYTE ACTIVITY — `next_json_alive` must consume comment-only
    /// chunks, keep the idle deadline alive, and still return the next
    /// data frame (the plain `next_json` under a timeout was blind to
    /// comments, so a quiet bus tripped the 45s watchdog every cycle).
    #[tokio::test]
    async fn alive_deadline_passes_heartbeats_to_data() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let activity = std::sync::Arc::new(AtomicUsize::new(0));
        let counter = activity.clone();
        let mut stream = SseStream {
            inner: Box::pin(futures::stream::iter(vec![
                Ok::<_, reqwest::Error>(bytes::Bytes::from_static(b": heartbeat\n\n")),
                Ok::<_, reqwest::Error>(bytes::Bytes::from_static(b": heartbeat\n\n")),
                Ok::<_, reqwest::Error>(bytes::Bytes::from_static(b"data: {\"a\":1}\n\n")),
            ])),
            buf: Vec::new(),
            done: false,
            cap: 64 * 1024,
            double_decode: false,
            on_activity: Some(Box::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
            })),
        };
        let value = stream
            .next_json_alive(std::time::Duration::from_millis(50))
            .await
            .expect("heartbeats keep the stream alive until the data frame")
            .expect("data frame");
        assert_eq!(value, serde_json::json!({"a": 1}));
        // Every received chunk — comments included — signals activity.
        assert_eq!(activity.load(Ordering::SeqCst), 3);
    }

    /// 2026-11-19 connection-flap regression, half 2: TRUE silence (no
    /// bytes at all — a half-open connection gone fully quiet) must
    /// still trip the byte-activity deadline.
    #[tokio::test]
    async fn alive_deadline_trips_on_silence() {
        let mut stream = SseStream {
            inner: Box::pin(futures::stream::pending::<Result<bytes::Bytes, reqwest::Error>>()),
            buf: Vec::new(),
            done: false,
            cap: 64 * 1024,
            double_decode: false,
            on_activity: None,
        };
        let err = stream
            .next_json_alive(std::time::Duration::from_millis(50))
            .await
            .expect_err("a silent stream must stall");
        assert!(err.to_string().contains("stalled"), "unexpected error: {err}");
    }

    /// Stream end under [`next_json_alive`] mirrors [`SseStream::next_json`]:
    /// `Ok(None)`, not a stall.
    #[tokio::test]
    async fn alive_deadline_stream_end_returns_none() {
        let mut stream = SseStream {
            inner: Box::pin(futures::stream::iter(
                Vec::<Result<bytes::Bytes, reqwest::Error>>::new(),
            )),
            buf: Vec::new(),
            done: false,
            cap: 64 * 1024,
            double_decode: false,
            on_activity: None,
        };
        assert!(stream
            .next_json_alive(std::time::Duration::from_millis(50))
            .await
            .expect("clean stream end")
            .is_none());
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
