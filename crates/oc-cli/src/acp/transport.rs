//! The ndjson transport with a message-size cap (the Rust stand-in for
//! the TS SDK's `ndJsonStream`, commands/handlers/acp.ts:27): newline-
//! delimited JSON-RPC over blocking `Read`/`Write` handles on dedicated
//! threads, rejecting lines over [`MAX_MESSAGE_BYTES`] the way v2's
//! `MessageTooLargeError` does (acp.ts:29-35).

use std::io::{Read, Write};
use std::sync::mpsc;
use std::thread;

use agent_client_protocol::{ConnectTo, Lines, Role};
use futures::channel::mpsc as futures_mpsc;
use futures::StreamExt;

/// The incoming-message cap. v2's TS SDK `ndJsonStream` enforces
/// `maxMessageBytes` and the acp handler reports it in MiB
/// (commands/handlers/acp.ts:31-32); the Rust SDK has no built-in cap,
/// so this port pins the same order of limit.
pub const MAX_MESSAGE_BYTES: usize = 35 * 1024 * 1024;

/// A capped ndjson transport over any blocking reader/writer pair.
pub struct CappedNdJson {
    sink: WriterSink,
    stream: LineStream,
}

impl CappedNdJson {
    /// Over this process's own stdin/stdout (the `opencode acp` binary).
    #[must_use]
    pub fn std_io() -> Self {
        Self::new(std::io::stdin(), std::io::stdout())
    }

    /// Over arbitrary blocking handles (the integration test drives a
    /// child's pipes).
    #[must_use]
    pub fn new<R, W>(read: R, write: W) -> Self
    where
        R: Read + Send + 'static,
        W: Write + Send + 'static,
    {
        Self { sink: WriterSink::spawn(write), stream: LineStream::spawn(read, MAX_MESSAGE_BYTES) }
    }
}

impl<R: Role> ConnectTo<R> for CappedNdJson {
    async fn connect_to(
        self,
        client: impl ConnectTo<R::Counterpart>,
    ) -> Result<(), agent_client_protocol::Error> {
        ConnectTo::<R>::connect_to(Lines::new(self.sink, self.stream), client).await
    }
}

type LinesResult = std::io::Result<String>;

/// An incoming line stream fed by a dedicated reader thread.
pub struct LineStream {
    rx: futures_mpsc::UnboundedReceiver<LinesResult>,
}

impl LineStream {
    fn spawn<R: Read + Send + 'static>(read: R, cap: usize) -> Self {
        let (tx, rx) = futures_mpsc::unbounded();
        let debug = std::env::var("OPENCODE_ACP_DEBUG").is_ok();
        thread::spawn(move || {
            let mut reader = std::io::BufReader::new(read);
            let mut bytes: Vec<u8> = Vec::new();
            loop {
                bytes.clear();
                // Read until '\n' with a running byte cap: byte-small
                // reads so an oversized line is detected before it is
                // buffered.
                let mut too_large = false;
                let mut done = false;
                loop {
                    let mut byte = [0u8; 1];
                    match reader.read(&mut byte) {
                        Ok(0) => {
                            done = true;
                            break;
                        }
                        Ok(_) => {
                            if bytes.len() + 1 > cap {
                                too_large = true;
                                break;
                            }
                            bytes.push(byte[0]);
                            if byte[0] == b'\n' {
                                break;
                            }
                        }
                        Err(error) => {
                            let _ = tx.unbounded_send(Err(error));
                            return;
                        }
                    }
                }
                if too_large {
                    let _ = tx.unbounded_send(Err(std::io::Error::other(format!(
                        "message too large: incoming message exceeds {cap} bytes"
                    ))));
                    return;
                }
                if !bytes.is_empty() {
                    while matches!(bytes.last(), Some(b'\n' | b'\r')) {
                        bytes.pop();
                    }
                    let trimmed = String::from_utf8_lossy(&bytes).into_owned();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if debug {
                        eprintln!("[acp-wire] <- {trimmed}");
                    }
                    if tx.unbounded_send(Ok(trimmed)).is_err() {
                        return;
                    }
                }
                if done {
                    return;
                }
                let _ = &bytes;
            }
        });
        Self { rx }
    }
}

impl futures::Stream for LineStream {
    type Item = LinesResult;
    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.rx.poll_next_unpin(cx)
    }
}

/// An outgoing line sink drained by a dedicated writer thread.
pub struct WriterSink {
    tx: mpsc::Sender<String>,
}

impl WriterSink {
    fn spawn<W: Write + Send + 'static>(write: W) -> Self {
        let (tx, rx) = mpsc::channel::<String>();
        thread::spawn(move || drain(write, rx));
        Self { tx }
    }
}

fn drain<W: Write>(mut write: W, rx: mpsc::Receiver<String>) {
    let debug = std::env::var("OPENCODE_ACP_DEBUG").is_ok();
    for line in rx {
        if debug {
            eprintln!("[acp-wire] -> {line}");
        }
        if write.write_all(line.as_bytes()).is_err() || write.write_all(b"\n").is_err() {
            return;
        }
        if write.flush().is_err() {
            return;
        }
    }
}

impl futures::Sink<String> for WriterSink {
    type Error = std::io::Error;
    fn poll_ready(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
    fn start_send(self: std::pin::Pin<&mut Self>, item: String) -> Result<(), Self::Error> {
        self.tx.send(item).map_err(|_| std::io::Error::other("writer thread gone"))
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The size-limit rejection (the MiB report, acp.ts:29-35).
    #[tokio::test]
    async fn oversized_lines_fail_with_the_limit_message() {
        let oversize = format!("{}\n", "x".repeat(33)).into_bytes();
        let mut stream = LineStream::spawn(std::io::Cursor::new(oversize), 32);
        let error = stream.next().await.expect("an item").expect_err("too large");
        assert!(error.to_string().contains("message too large"), "unexpected error: {error}");
        assert!(stream.next().await.is_none(), "reader stops after the cap trips");
    }

    /// A normal-size line flows through and EOF ends the stream.
    #[tokio::test]
    async fn normal_lines_flow() {
        let input = b"{\"jsonrpc\":\"2.0\",\"method\":\"x\",\"params\":{}}\n".to_vec();
        let mut stream = LineStream::spawn(std::io::Cursor::new(input), MAX_MESSAGE_BYTES);
        let line = stream.next().await.expect("line").expect("ok");
        assert!(line.starts_with("{\"jsonrpc\""));
        assert!(stream.next().await.is_none(), "eof after one line");
    }

    /// CRLF and blank lines are tolerated.
    #[tokio::test]
    async fn crlf_lines_are_trimmed() {
        let input = b"a\r\n\r\nb\r\n".to_vec();
        let mut stream = LineStream::spawn(std::io::Cursor::new(input), MAX_MESSAGE_BYTES);
        assert_eq!(stream.next().await.unwrap().unwrap(), "a");
        assert_eq!(stream.next().await.unwrap().unwrap(), "b");
        assert!(stream.next().await.is_none());
    }
}
