//! Response framing: bytes → protocol frames (port of `src/route/framing.ts`).
//!
//! The SSE decoder is incremental and survives every edge in the recorded
//! corpus (oracle gate 2, finding 18): multi-line `data:` payloads, `event:`
//! names, comment lines and heartbeats, chunk boundaries **mid-frame and
//! mid-UTF-8**, `[DONE]` sentinels, `retry:` directives, `data: null` flush
//! markers, and oversized-event failures surfacing as typed AI errors.

use std::collections::BTreeSet;

use crate::schema::{AiError, AiErrorReason, ReasonFields};

/// Upper bound on one buffered SSE event (matches the Effect SSE decoder's
/// limit; larger frames are provider defects, not payloads).
pub const MAX_EVENT_BYTES: usize = 10 * 1024 * 1024;

/// Framing flavor (port of the `Framing` definitions).
#[derive(Clone, Debug)]
pub enum FramingKind {
    /// Server-Sent Events; drops `[DONE]`, `data: null`, and `: keepalive`
    /// data markers. Optionally restricted to a set of `event:` names
    /// (Anthropic) and/or retaining `[DONE]` (OpenAI chat).
    Sse { include_done: bool, events: Option<BTreeSet<String>> },
    /// Newline-delimited JSON (NDJSON); blank lines dropped.
    Lines,
    /// The whole body as one frame.
    Document,
}

/// One framing definition: the byte-stream-shaped seam between transport and
/// protocol (port of `Framing.Definition`).
#[derive(Clone, Debug)]
pub struct Framing {
    pub id: &'static str,
    pub kind: FramingKind,
}

impl Framing {
    /// Server-Sent Events framing used by every JSON-streaming HTTP provider.
    pub fn sse() -> Self {
        Self { id: "sse", kind: FramingKind::Sse { include_done: false, events: None } }
    }

    /// SSE framing that retains the conventional `[DONE]` sentinel.
    pub fn sse_with_done() -> Self {
        Self { id: "sse", kind: FramingKind::Sse { include_done: true, events: None } }
    }

    /// SSE framing restricted to protocol-recognized `event:` names.
    pub fn sse_events(names: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            id: "sse",
            kind: FramingKind::Sse { include_done: false, events: Some(names.into_iter().map(Into::into).collect()) },
        }
    }

    /// Newline-delimited JSON.
    pub fn lines() -> Self {
        Self { id: "lines", kind: FramingKind::Lines }
    }

    /// One frame for the whole body.
    pub fn document() -> Self {
        Self { id: "document", kind: FramingKind::Document }
    }
}

/// Incremental UTF-8 decoder that survives chunk boundaries inside a
/// multi-byte sequence.
#[derive(Default)]
struct Utf8Decoder {
    pending: Vec<u8>,
}

impl Utf8Decoder {
    /// Feed raw bytes; returns decoded text. Incomplete trailing sequences
    /// are buffered until the next chunk. Invalid UTF-8 is an error (the
    /// transport delivered corrupt data).
    fn feed(&mut self, chunk: &[u8]) -> Result<String, AiError> {
        let mut buffer = std::mem::take(&mut self.pending);
        buffer.extend_from_slice(chunk);
        match std::str::from_utf8(&buffer) {
            Ok(text) => Ok(text.to_string()),
            Err(error) => {
                let valid = error.valid_up_to();
                if error.error_len().is_none() {
                    // incomplete sequence at the end — hold it back
                    let text = String::from_utf8_lossy(&buffer[..valid]).into_owned();
                    self.pending = buffer[valid..].to_vec();
                    Ok(text)
                } else {
                    Err(AiError::new(AiErrorReason::InvalidProviderOutput {
                        fields: ReasonFields::new(format!("Invalid UTF-8 in provider stream: {error}")),
                        classification: None,
                        route: Some("sse".into()),
                    }))
                }
            }
        }
    }

    fn finish(&mut self) -> Result<String, AiError> {
        if self.pending.is_empty() {
            return Ok(String::new());
        }
        self.pending.clear();
        Err(AiError::new(AiErrorReason::InvalidProviderOutput {
            fields: ReasonFields::new("Truncated UTF-8 sequence at end of provider stream"),
            classification: None,
            route: Some("sse".into()),
        }))
    }
}

/// Incremental frames decoder over arbitrary chunk boundaries. Feed raw
/// response bytes; complete frames come back in order (port of
/// `sseFraming` + `Stream.splitLines`).
pub struct FramingDecoder {
    kind: FramingKind,
    utf8: Utf8Decoder,
    partial_line: String,
    // SSE event state
    event_name: String,
    data_lines: Vec<String>,
    buffered_bytes: usize,
    // document framing
    document: String,
    finished: bool,
}

impl FramingDecoder {
    pub fn new(framing: Framing) -> Self {
        Self {
            kind: framing.kind,
            utf8: Utf8Decoder::default(),
            partial_line: String::new(),
            event_name: String::new(),
            data_lines: Vec::new(),
            buffered_bytes: 0,
            document: String::new(),
            finished: false,
        }
    }

    /// Feed one chunk of raw response bytes.
    pub fn feed(&mut self, chunk: &[u8]) -> Result<Vec<String>, AiError> {
        match &self.kind {
            FramingKind::Document => {
                let text = self.utf8.feed(chunk)?;
                self.document.push_str(&text);
                Ok(Vec::new())
            }
            FramingKind::Lines => {
                let text = self.utf8.feed(chunk)?;
                self.partial_line.push_str(&text);
                Ok(self.take_complete_lines())
            }
            FramingKind::Sse { .. } => {
                let text = self.utf8.feed(chunk)?;
                self.partial_line.push_str(&text);
                let lines = self.take_complete_lines();
                self.consume_lines(lines)
            }
        }
    }

    /// Signal end-of-body: `document` framing emits its single frame; SSE
    /// events still missing their blank-line terminator are discarded
    /// (channel semantics: an incomplete final event never dispatched).
    pub fn finish(&mut self) -> Result<Vec<String>, AiError> {
        if self.finished {
            return Ok(Vec::new());
        }
        self.finished = true;
        match &self.kind {
            FramingKind::Document => {
                self.utf8.finish()?;
                Ok(vec![std::mem::take(&mut self.document)])
            }
            FramingKind::Lines => {
                self.utf8.finish()?;
                let remainder = std::mem::take(&mut self.partial_line);
                if remainder.trim().is_empty() {
                    Ok(Vec::new())
                } else {
                    Ok(vec![remainder])
                }
            }
            FramingKind::Sse { .. } => {
                self.utf8.finish()?;
                let remainder = std::mem::take(&mut self.partial_line);
                if remainder.is_empty() {
                    return Ok(Vec::new());
                }
                self.consume_lines(vec![remainder])
            }
        }
    }

    /// Take complete lines out of `partial_line`; keeps the trailing partial
    /// (no terminator yet). Lines terminate on `\n`, `\r\n`, or `\r`.
    fn take_complete_lines(&mut self) -> Vec<String> {
        let text = std::mem::take(&mut self.partial_line);
        let mut lines = Vec::new();
        let mut current = String::new();
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '\n' => lines.push(std::mem::take(&mut current)),
                '\r' => {
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    lines.push(std::mem::take(&mut current));
                }
                _ => current.push(ch),
            }
        }
        self.partial_line = current;
        lines
    }

    fn consume_lines(&mut self, lines: Vec<String>) -> Result<Vec<String>, AiError> {
        let mut frames = Vec::new();
        for line in lines {
            self.buffered_bytes = self.buffered_bytes.saturating_add(line.len());
            if self.buffered_bytes > MAX_EVENT_BYTES {
                return Err(AiError::new(AiErrorReason::InvalidProviderOutput {
                    fields: ReasonFields::new(format!(
                        "SSE event exceeded the {MAX_EVENT_BYTES} byte limit"
                    )),
                    classification: None,
                    route: Some("sse".into()),
                }));
            }
            if line.is_empty() {
                frames.extend(self.dispatch());
                continue;
            }
            if line.starts_with(':') {
                // comment / heartbeat line
                continue;
            }
            let (field, value) = match line.split_once(':') {
                Some((field, rest)) => (field, rest.strip_prefix(' ').unwrap_or(rest)),
                None => (line.as_str(), ""),
            };
            match field {
                "event" => self.event_name = value.to_string(),
                "data" => self.data_lines.push(value.to_string()),
                // `retry:` and unknown fields are ignored without ending the stream
                _ => {}
            }
        }
        Ok(frames)
    }

    /// Dispatch the pending event, applying the ported filter
    /// (framing.ts:47-61). Resets the pending buffers.
    fn dispatch(&mut self) -> Vec<String> {
        self.buffered_bytes = 0;
        let name = if self.event_name.is_empty() { "message" } else { self.event_name.as_str() };
        let data = std::mem::take(&mut self.data_lines).join("\n");
        self.event_name.clear();
        let mut frames = Vec::new();
        if let FramingKind::Sse { include_done, events } = &self.kind {
            let named_ok = events.as_ref().map(|set| set.contains(name)).unwrap_or(true);
            let passes = named_ok
                && !data.is_empty()
                // Some OpenAI-compatible proxies serialize an empty flush as a
                // bare `data: null` between events or after `[DONE]`.
                && data != "null"
                // Vertex AI partner models send `data: : keepalive` while reasoning.
                && data != ": keepalive"
                && (data != "[DONE]" || *include_done || events.as_ref().is_some_and(|_| name != "message"));
            if passes {
                frames.push(data);
            }
        }
        frames
    }
}

/// Stream adapter: raw byte chunks → frames. This is the shape the route
/// pipeline consumes; `decode` errors surface as `InvalidProviderOutput`.
pub fn frame_stream(
    framing: Framing,
    mut bytes: impl futures::Stream<Item = Result<bytes::Bytes, crate::route::transport::TransportError>>
        + Send
        + Unpin
        + 'static,
) -> impl futures::Stream<Item = Result<String, AiError>> + Send + 'static {
    async_stream::stream! {
        let mut decoder = FramingDecoder::new(framing);
        use futures::StreamExt;
        while let Some(chunk) = bytes.next().await {
            match chunk {
                Ok(chunk) => match decoder.feed(&chunk) {
                    Ok(frames) => { for frame in frames { yield Ok(frame); } }
                    Err(error) => { yield Err(error); return; }
                },
                Err(error) => {
                    yield Err(crate::route::transport::transport_read_error(error));
                    return;
                }
            }
        }
        match decoder.finish() {
            Ok(frames) => { for frame in frames { yield Ok(frame); } }
            Err(error) => yield Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_all(framing: Framing, input: &str) -> Vec<String> {
        let mut decoder = FramingDecoder::new(framing);
        let mut frames = decoder.feed(input.as_bytes()).unwrap();
        frames.extend(decoder.finish().unwrap());
        frames
    }

    fn decode_chunks(framing: Framing, chunks: &[&str]) -> Vec<String> {
        let mut decoder = FramingDecoder::new(framing);
        let mut frames = Vec::new();
        for chunk in chunks {
            frames.extend(decoder.feed(chunk.as_bytes()).unwrap());
        }
        frames.extend(decoder.finish().unwrap());
        frames
    }

    #[test]
    fn sse_drops_done_null_and_keepalive_markers() {
        let input = concat!(
            "data: {\"first\":true}\n\n",
            "data: null\n\n",
            "event: update\ndata: null\n\n",
            "data: \"null\"\n\n",
            "data: 0\n\n",
            "data: {\"second\":true}\n\n",
            "data: [DONE]\n\n",
            "data: null\n\n",
        );
        assert_eq!(
            decode_all(Framing::sse(), input),
            vec!["{\"first\":true}", "\"null\"", "0", "{\"second\":true}"]
        );
    }

    #[test]
    fn sse_drops_keepalive_comments_sent_as_data() {
        let input = concat!(
            "data: {\"first\":true}\n\n",
            "data: : keepalive\n\n",
            ": keepalive\n\n",
            "data: : ping\n\n",
            "data: {\"second\":true}\n\n",
        );
        assert_eq!(
            decode_all(Framing::sse(), input),
            vec!["{\"first\":true}", ": ping", "{\"second\":true}"]
        );
    }

    #[test]
    fn sse_ignores_retry_directives_without_ending_the_stream() {
        let chunks = ["retry: 1000\n\n", "data: {\"first\":true}\n\n", "retry: 2000\n\n", "data: {\"second\":true}\n\n"];
        assert_eq!(decode_chunks(Framing::sse(), &chunks), vec!["{\"first\":true}", "{\"second\":true}"]);
    }

    #[test]
    fn sse_preserves_event_data_around_retry_directives() {
        let chunks = ["event: update\ndata: first\n", "retry: 1000\n", "data: second\n\n"];
        assert_eq!(
            decode_chunks(Framing::sse_events(["update"]), &chunks),
            vec!["first\nsecond"]
        );
    }

    #[test]
    fn sse_with_done_retains_the_sentinel() {
        let input = "data: {\"a\":1}\n\ndata: [DONE]\n\n";
        assert_eq!(decode_all(Framing::sse_with_done(), input), vec!["{\"a\":1}", "[DONE]"]);
        assert_eq!(decode_all(Framing::sse(), input), vec!["{\"a\":1}"]);
    }

    #[test]
    fn multi_line_data_joins_with_newlines() {
        let input = "data: line1\ndata: line2\n\n";
        assert_eq!(decode_all(Framing::sse(), input), vec!["line1\nline2"]);
    }

    #[test]
    fn unnamed_events_are_filtered_out_by_sse_events() {
        let input = "data: dropped\n\nevent: message_start\ndata: {\"ok\":1}\n\n";
        assert_eq!(decode_all(Framing::sse_events(["message_start"]), input), vec!["{\"ok\":1}"]);
    }

    #[test]
    fn done_sent_with_a_named_event_survives_sse_events_filtering() {
        let input = "event: completion\ndata: [DONE]\n\n";
        assert_eq!(decode_all(Framing::sse_events(["completion"]), input), vec!["[DONE]"]);
    }

    #[test]
    fn comment_lines_and_heartbeats_are_ignored() {
        let input = ": o hai\nevent: ping\ndata: {}\n\n: another comment\n\ndata: {\"next\":1}\n\n";
        assert_eq!(decode_all(Framing::sse_events(["ping"]), input), vec!["{}"]);
    }

    #[test]
    fn crlf_and_lone_cr_line_endings_terminate_lines() {
        let input = "event: ping\r\ndata: one\r\rdata: two\n\n";
        assert_eq!(decode_all(Framing::sse_events(["ping"]), input), vec!["one", "two"]);
    }

    #[test]
    fn chunk_boundaries_mid_frame_and_mid_utf8_preserve_frames() {
        // deterministic permutations of one byte-at-a-time and boundary splits
        let input = "event: ping\ndata: {\"snowman\":\"☃\"}\n\n: c\ndata: ☃☃\n\n".to_string();
        let whole = decode_all(Framing::sse_events(["ping"]), &input);
        assert_eq!(whole, vec!["{\"snowman\":\"☃\"}", "☃☃"]);

        // every single-byte split point must yield the same frames
        let bytes = input.as_bytes();
        for split in 0..bytes.len() {
            let (a, b) = bytes.split_at(split);
            let mut decoder = FramingDecoder::new(Framing::sse_events(["ping"]));
            let mut frames = decoder.feed(a).unwrap();
            frames.extend(decoder.feed(b).unwrap());
            frames.extend(decoder.finish().unwrap());
            assert_eq!(frames, whole, "mismatch at byte split {split}");
        }
    }

    #[test]
    fn chunk_boundaries_inside_multi_byte_characters_are_decoded_once() {
        let emoji = "😀".to_string(); // 4-byte UTF-8 sequence
        let payload = format!("data: {emoji}\n\n");
        let bytes = payload.as_bytes();
        // split inside the emoji (byte 6 is the first content byte of 😀 after "data: ")
        for split in 6..(6 + emoji.len()) {
            let (a, b) = bytes.split_at(split);
            let mut decoder = FramingDecoder::new(Framing::sse());
            let mut frames = decoder.feed(a).unwrap();
            frames.extend(decoder.feed(b).unwrap());
            frames.extend(decoder.finish().unwrap());
            assert_eq!(frames, vec![emoji.as_str()], "mismatch at split {split}");
        }
    }

    #[test]
    fn oversize_events_map_to_invalid_provider_output() {
        let mut decoder = FramingDecoder::new(Framing::sse());
        let huge = format!("data: {}\n\n", "x".repeat(MAX_EVENT_BYTES + 1));
        let error = decoder.feed(huge.as_bytes()).unwrap_err();
        assert!(matches!(error.reason, AiErrorReason::InvalidProviderOutput { .. }));
    }

    #[test]
    fn invalid_utf8_maps_to_invalid_provider_output() {
        let mut decoder = FramingDecoder::new(Framing::sse());
        let error = decoder.feed(&[0x64, 0x61, 0x74, 0x61, 0x3a, 0x20, 0xff, 0x0a, 0x0a]).unwrap_err();
        assert!(matches!(error.reason, AiErrorReason::InvalidProviderOutput { .. }));
    }

    #[test]
    fn incomplete_events_at_eof_are_discarded() {
        // data without a trailing blank line never dispatches
        assert_eq!(decode_all(Framing::sse(), "data: dangling"), Vec::<String>::new());
    }

    #[test]
    fn ndjson_lines_drop_blank_lines() {
        let input = "{\"a\":1}\n\n{\"b\":2}\n   \n{\"c\":3}";
        assert_eq!(decode_all(Framing::lines(), input), vec!["{\"a\":1}", "{\"b\":2}", "{\"c\":3}"]);
    }

    #[test]
    fn document_framing_emits_one_frame() {
        let mut decoder = FramingDecoder::new(Framing::document());
        assert!(decoder.feed(b"{\"part\":").unwrap().is_empty());
        assert!(decoder.feed(b"1}").unwrap().is_empty());
        assert_eq!(decoder.finish().unwrap(), vec!["{\"part\":1}".to_string()]);
    }

    #[test]
    fn frame_stream_maps_transport_read_errors_to_ai_errors() {
        use futures::StreamExt;
        let error = crate::route::transport::TransportError::connection(
            "socket disconnected".into(),
            crate::route::transport::TransportOperation::Read,
        );
        let stream = frame_stream(Framing::sse(), futures::stream::iter(vec![Err(error)]));
        let collected: Vec<Result<String, AiError>> = futures::executor::block_on(stream.collect());
        assert_eq!(collected.len(), 1);
        let error = collected[0].as_ref().unwrap_err();
        assert!(matches!(
            error.reason,
            AiErrorReason::Transport { operation: crate::schema::errors::TransportOperation::Read, .. }
        ));
    }
}
