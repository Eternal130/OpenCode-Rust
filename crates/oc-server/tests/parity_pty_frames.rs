//! WIRE-cluster parity tests (ledger C4a): PTY websocket frame semantics.
//!
//! - PTY-F1 `meta_after_replay_binary` — v2 streams replay chunks (64 KiB)
//!   FIRST, then one binary control frame `0x00 || {"cursor":N}`
//!   (`packages/core/src/pty/protocol.ts:14-31`,
//!   `packages/server/src/handlers/pty.ts:198-199`).
//! - PTY-F2 `normal_exit_close_1000` — normal PTY exit closes with 1000;
//!   4404 ("session exited") is the attach-failure code
//!   (`handlers/pty.ts:169-199`).
//! - PTY-F4 `origin_gate_no_consume` — a ticket is consumed only when the
//!   Origin gate passes; a disallowed Origin gets 403 and leaves the
//!   ticket valid (`handlers/pty.ts:153-159`).
//! - PTY-F3 `resized_checkpoint` — the persistent-PTY `resized` frame
//!   carries the post-resize emulator checkpoint (base64) and generation
//!   (`handlers/persistent-pty.ts:141-145`).

use std::fmt::Write as _;
use std::sync::Arc;

use oc_client::types::*;
use oc_client::Client;
use oc_core::engine::{Engine, EngineOptions};
use oc_server::handlers;
use oc_server::state::{AppState, AuthConfig};
use oc_util::determinism::{SystemClock, UuidV4IdGen};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

struct TestServer {
    client: Client,
    addr: std::net::SocketAddr,
    /// The workspace root. Kept (with its directory guard) so spawned
    /// ptys keep a valid cwd for the test's lifetime.
    #[allow(dead_code)] // the guard is the point; readers may come and go
    workspace: std::path::PathBuf,
    #[allow(dead_code)] // directory guard for `workspace`
    _dir: tempfile::TempDir,
    #[allow(dead_code)] // engine state root guard
    _data: tempfile::TempDir,
}

async fn boot() -> TestServer {
    // Hermetic (R3): persistent-terminal tests pin the in-process
    // backend — the one-release OC_PTY_DAEMON=0 escape hatch.
    std::env::set_var("OC_PTY_DAEMON", "0");
    let dir = tempfile::tempdir().expect("tempdir");
    let data = tempfile::tempdir().expect("data tempdir");
    let workspace = dir.path().to_path_buf();
    std::env::set_current_dir(&workspace).expect("chdir to workspace");
    let engine =
        Engine::boot(EngineOptions::new(data.path().join("data"), data.path().join("home"))).await;
    let state = Arc::new(AppState::new(
        engine,
        AuthConfig { password: None },
        "test-version".to_owned(),
        Arc::new(UuidV4IdGen),
        Arc::new(SystemClock),
    ));
    let router = handlers::mount(Arc::clone(&state));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("ephemeral bind");
    let addr = listener.local_addr().expect("addr");
    let _ = state.local_addr.write().insert(addr);
    let _ = state.readiness.send(oc_server::state::Readiness::Ready);
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    let client = Client::new(&format!("http://{addr}")).expect("client");
    TestServer { client, addr, workspace, _dir: dir, _data: data }
}

async fn create_session(client: &Client, title: &str) -> String {
    client
        .session()
        .create(&SessionCreateBody { title: Some(title.to_owned()), ..Default::default() })
        .await
        .expect("create")
        .id
}

/// Raw HTTP helper returning `(status, body_json)`.
async fn raw_http(
    addr: std::net::SocketAddr,
    method: &str,
    target: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> (u16, Value, String) {
    let mut stream = TcpStream::connect(addr).await.expect("connect");
    let mut request = format!(
        "{method} {target} HTTP/1.1\r\nhost: {addr}\r\nconnection: close\r\ncontent-length: {}\r\n",
        body.len()
    );
    for (name, value) in headers {
        let _ = writeln!(request, "{name}: {value}");
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes()).await.expect("write head");
    stream.write_all(body).await.expect("write body");
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.expect("read");
    let text = String::from_utf8_lossy(&raw).into_owned();
    let status =
        text.split_whitespace().nth(1).and_then(|code| code.parse::<u16>().ok()).unwrap_or(0);
    let body_text =
        text.split_once("\r\n\r\n").map(|(_, rest)| rest.to_owned()).unwrap_or_default();
    let json = serde_json::from_str(&body_text).unwrap_or(Value::Null);
    (status, json, body_text)
}

/// Creates a PTY through the production REST route.
async fn create_pty(server: &TestServer, command: &str, args: &[&str]) -> String {
    let (status, body, _) = raw_http(
        server.addr,
        "POST",
        "/api/pty",
        &[("content-type", "application/json")],
        serde_json::json!({ "command": command, "args": args }).to_string().as_bytes(),
    )
    .await;
    assert_eq!(status, 200, "pty create: {body}");
    body["data"]["id"].as_str().expect("pty id").to_owned()
}

// ---------------------------------------------------------------------------
// Minimal RFC6455 client (client frames masked; no extensions)
// ---------------------------------------------------------------------------

/// The largest payload a u16 extended length can carry.
const U16_MAX_LEN: usize = u16::MAX as usize;

pub struct WsFrame {
    pub opcode: u8,
    pub payload: Vec<u8>,
}

struct RawFrame {
    fin: bool,
    opcode: u8,
    payload: Vec<u8>,
}

pub struct WsClient {
    stream: TcpStream,
    buf: Vec<u8>,
}

/// Performs the upgrade handshake; `None` + status when it was rejected.
pub async fn ws_connect(
    addr: std::net::SocketAddr,
    target: &str,
    origin: Option<&str>,
) -> std::io::Result<(u16, Option<WsClient>)> {
    let mut stream = TcpStream::connect(addr).await?;
    // A fixed 16-byte key: the server only needs a decodable base64 value.
    let key = "dGhpcyBpcyBhIHRlc3Qga2V5IQ==";
    let mut request = format!(
        "GET {target} HTTP/1.1\r\nhost: {addr}\r\nupgrade: websocket\r\nconnection: \
         Upgrade\r\nsec-websocket-key: {key}\r\nsec-websocket-version: 13\r\n"
    );
    if let Some(origin) = origin {
        let _ = write!(request, "origin: {origin}\r\n");
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes()).await?;
    let mut head = Vec::new();
    let mut chunk = [0u8; 2048];
    loop {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "no head"));
        }
        head.extend_from_slice(&chunk[..read]);
        if let Some(position) = head.windows(4).position(|window| window == b"\r\n\r\n") {
            let text = String::from_utf8_lossy(&head[..position]).into_owned();
            let status = text
                .split_whitespace()
                .nth(1)
                .and_then(|code| code.parse::<u16>().ok())
                .unwrap_or(0);
            if status != 101 {
                return Ok((status, None));
            }
            return Ok((101, Some(WsClient { stream, buf: head[position + 4..].to_vec() })));
        }
    }
}

impl WsClient {
    /// Sends one masked text frame.
    #[allow(dead_code)] // kept for input-path assertions in follow-up lanes
    pub async fn send_text(&mut self, text: &str) -> std::io::Result<()> {
        self.send_frame(0x1, text.as_bytes()).await
    }

    async fn send_frame(&mut self, opcode: u8, payload: &[u8]) -> std::io::Result<()> {
        use rand::Rng as _;
        let mask: [u8; 4] = rand::thread_rng().gen();
        let mut frame = vec![0x80 | opcode];
        let len = payload.len();
        #[allow(clippy::cast_possible_truncation)] // the arms bound the ranges
        if len < 126 {
            frame.push(0x80 | len as u8);
        } else if len <= U16_MAX_LEN {
            frame.push(0xFE); // masked client frame + u16 extended length
            frame.extend_from_slice(&(len as u16).to_be_bytes());
        } else {
            frame.push(0xFF); // masked client frame + u64 extended length
            frame.extend_from_slice(&(len as u64).to_be_bytes());
        }
        frame.extend_from_slice(&mask);
        for (index, byte) in payload.iter().enumerate() {
            frame.push(byte ^ mask[index % 4]);
        }
        self.stream.write_all(&frame).await
    }

    /// Reads the next complete (defragmented) frame; `None` on timeout/EOF.
    pub async fn recv(&mut self, limit: std::time::Duration) -> Option<WsFrame> {
        let deadline = tokio::time::Instant::now() + limit;
        let mut opcode = 0u8;
        let mut payload = Vec::new();
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return None;
            }
            let raw = match tokio::time::timeout(remaining, self.read_frame()).await {
                Ok(frame) => frame?,
                Err(_) => return None,
            };
            if opcode == 0 {
                opcode = raw.opcode;
            }
            payload.extend_from_slice(&raw.payload);
            if raw.fin {
                return Some(WsFrame { opcode, payload });
            }
        }
    }

    /// Reads one raw frame off the socket.
    async fn read_frame(&mut self) -> Option<RawFrame> {
        loop {
            if self.buf.len() >= 2 {
                let fin = self.buf[0] & 0x80 != 0;
                let opcode = self.buf[0] & 0x0F;
                let masked = self.buf[1] & 0x80 != 0;
                let len7 = (self.buf[1] & 0x7F) as usize;
                let header = match len7 {
                    126 => 4,
                    127 => 10,
                    _ => 2,
                };
                if self.buf.len() >= header {
                    let len = match len7 {
                        126 => usize::from(u16::from_be_bytes([self.buf[2], self.buf[3]])),
                        127 => usize::try_from(u64::from_be_bytes(
                            self.buf[2..10].try_into().expect("8 bytes"),
                        ))
                        .ok()?,
                        other => other,
                    };
                    let mask_len = usize::from(masked) * 4;
                    if self.buf.len() >= header + mask_len + len {
                        let mask_start = header;
                        let payload_start = header + mask_len;
                        let mut payload = self.buf[payload_start..payload_start + len].to_vec();
                        if masked {
                            let mask = self.buf[mask_start..mask_start + 4].to_vec();
                            for (index, byte) in payload.iter_mut().enumerate() {
                                *byte ^= mask[index % 4];
                            }
                        }
                        self.buf.drain(..payload_start + len);
                        return Some(RawFrame { fin, opcode, payload });
                    }
                }
            }
            let mut chunk = [0u8; 8192];
            let read = self.stream.read(&mut chunk).await.ok()?;
            if read == 0 {
                return None;
            }
            self.buf.extend_from_slice(&chunk[..read]);
        }
    }
}

impl WsFrame {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.payload).into_owned()
    }
}

fn close_code(frame: &WsFrame) -> u16 {
    assert_eq!(frame.opcode, 0x8, "expected close frame, got opcode {}", frame.opcode);
    u16::from_be_bytes([frame.payload[0], frame.payload[1]])
}

fn close_reason(frame: &WsFrame) -> String {
    String::from_utf8_lossy(&frame.payload[2..]).into_owned()
}

// ---------------------------------------------------------------------------
// PTY-F1 — replay first (64 KiB chunks), then the binary meta frame
// ---------------------------------------------------------------------------

#[tokio::test]
async fn meta_after_replay_binary() {
    let server = boot().await;
    // ~100 KiB of output, then the process keeps running so the attach
    // always sees the complete retained buffer as replay.
    let pty_id = create_pty(&server, "sh", &["-c", "yes A | head -c 100000; sleep 30"]).await;
    // Let the pipe settle before attaching (the write itself is instant).
    tokio::time::sleep(std::time::Duration::from_millis(750)).await;

    let (status, socket) =
        ws_connect(server.addr, &format!("/api/pty/{pty_id}/connect"), None).await.expect("ws");
    assert_eq!(status, 101);
    let mut socket = socket.expect("upgraded");

    // Replay frames: every chunk ≤ 64 KiB, all but the last exactly
    // 64 KiB (the pty's ONLCR may expand the written bytes), then the
    // binary meta whose cursor equals the replayed byte count.
    let mut replayed = 0usize;
    let mut sizes: Vec<usize> = Vec::new();
    let meta = loop {
        let frame = socket.recv(std::time::Duration::from_secs(10)).await.expect("replay/meta");
        if frame.opcode == 0x2 {
            break frame;
        }
        assert_eq!(frame.opcode, 0x1, "replay rides text frames");
        assert!(
            frame.payload.len() <= 64 * 1024,
            "replay chunks are bounded by 64 KiB: {}",
            frame.payload.len()
        );
        replayed += frame.payload.len();
        sizes.push(frame.payload.len());
    };
    assert!(sizes.len() >= 2, "100 KiB of output splits into 64 KiB chunks ({sizes:?})");
    assert!(
        sizes[..sizes.len() - 1].iter().all(|size| *size == 64 * 1024),
        "all but the final chunk are exactly 64 KiB: {sizes:?}"
    );

    // The meta frame — BINARY, 0x00-prefixed {"cursor":N}.
    assert_eq!(meta.payload[0], 0x00, "meta frame starts with the 0x00 control byte");
    let meta_json: Value =
        serde_json::from_slice(&meta.payload[1..]).expect("meta payload is JSON after 0x00");
    assert!(
        meta_json.get("cursor").is_some_and(Value::is_u64),
        "meta carries the absolute cursor: {meta_json}"
    );
    assert!(
        meta_json.get("type").is_none(),
        "v2 meta frames carry only {{cursor}} (no type key): {meta_json}"
    );
    assert_eq!(meta_json["cursor"].as_u64(), Some(replayed as u64), "cursor after the full replay");
}

// ---------------------------------------------------------------------------
// PTY-F2 — close codes: 1000 normal exit, 4404 attach failures
// ---------------------------------------------------------------------------

#[tokio::test]
async fn normal_exit_close_1000() {
    let server = boot().await;
    // Normal exit: the process runs, prints, and exits zero while attached
    // (the sleep guards the upgrade handshake against a lost race).
    let pty_id = create_pty(&server, "sh", &["-c", "echo hi; sleep 1; exit 0"]).await;
    let (status, socket) =
        ws_connect(server.addr, &format!("/api/pty/{pty_id}/connect"), None).await.expect("ws");
    assert_eq!(status, 101);
    let mut socket = socket.expect("upgraded");
    let close = loop {
        let frame = socket.recv(std::time::Duration::from_secs(20)).await.expect("frame");
        if frame.opcode == 0x8 {
            break frame;
        }
    };
    assert_eq!(close_code(&close), 1000, "normal exit closes with 1000");

    // Attach failure on an exited PTY: 4404 "session exited" (v2 maps
    // Pty.ExitedError → CloseEvent(4404, "session exited")).
    let exited_id = create_pty(&server, "sh", &["-c", "exit 0"]).await;
    for _ in 0..200 {
        let (_, info, _) =
            raw_http(server.addr, "GET", &format!("/api/pty/{exited_id}"), &[], &[]).await;
        if info["data"]["status"] == "exited" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let (status, socket) =
        ws_connect(server.addr, &format!("/api/pty/{exited_id}/connect"), None).await.expect("ws");
    assert_eq!(status, 101, "exited ptys upgrade; the failure is a close frame");
    let mut socket = socket.expect("upgraded");
    let close = socket.recv(std::time::Duration::from_secs(10)).await.expect("close frame");
    assert_eq!(close_code(&close), 4404);
    assert_eq!(close_reason(&close), "session exited");

    // Unknown PTY: v2 returns an empty 404 before upgrading.
    let (status, socket) =
        ws_connect(server.addr, "/api/pty/pty_missing/connect", None).await.expect("ws");
    assert_eq!(status, 404, "unknown pty is a plain 404");
    assert!(socket.is_none());
}

// ---------------------------------------------------------------------------
// PTY-F4 — Origin gate before ticket consumption
// ---------------------------------------------------------------------------

#[tokio::test]
async fn origin_gate_no_consume() {
    let server = boot().await;
    let pty_id = create_pty(&server, "sh", &["-c", "sleep 30"]).await;
    let host = server.addr.to_string();

    // Issue a ticket through the production connect-token route.
    let (status, issued, _) = raw_http(
        server.addr,
        "POST",
        &format!("/api/pty/{pty_id}/connect-token"),
        &[("x-opencode-ticket", "1"), ("origin", &format!("http://{host}"))],
        &[],
    )
    .await;
    assert_eq!(status, 200, "connect-token: {issued}");
    let ticket = issued["data"]["ticket"].as_str().expect("ticket string").to_owned();

    // Disallowed Origin → 403 and the ticket is NOT consumed.
    let (status, socket) = ws_connect(
        server.addr,
        &format!("/api/pty/{pty_id}/connect?ticket={ticket}"),
        Some("http://evil.example"),
    )
    .await
    .expect("ws");
    assert_eq!(status, 403, "disallowed origin must be rejected before upgrade");
    assert!(socket.is_none());

    // Same ticket, allowed Origin: still consumable (the 403 did not burn it).
    let (status, socket) = ws_connect(
        server.addr,
        &format!("/api/pty/{pty_id}/connect?ticket={ticket}"),
        Some(&format!("http://{host}")),
    )
    .await
    .expect("ws");
    assert_eq!(status, 101, "unconsumed ticket upgrades cleanly");
    assert!(socket.is_some());

    // Single-use: the consumed ticket cannot be replayed.
    let (status, _) = ws_connect(
        server.addr,
        &format!("/api/pty/{pty_id}/connect?ticket={ticket}"),
        Some(&format!("http://{host}")),
    )
    .await
    .expect("ws");
    assert_eq!(status, 403, "tickets are single-use");
}

// ---------------------------------------------------------------------------
// PTY-F3 — persistent-PTY resized frames carry the checkpoint
// ---------------------------------------------------------------------------

#[tokio::test]
async fn resized_checkpoint() {
    let server = boot().await;
    let session_id = create_session(&server.client, "f3").await;
    let (status, created, _) = raw_http(
        server.addr,
        "POST",
        &format!("/api/experimental/session/{session_id}/terminal"),
        &[("content-type", "application/json")],
        // `env` is REQUIRED by PersistentPty.CreateInput (probe-pinned
        // 2026-10-20: v2.0.24 400s env-less creates with `Missing key
        // at ["env"]`; its own tests always send `env: {}` —
        // packages/server/test/persistent-pty.test.ts:47,168,196). The
        // pre-batch-2 payload here was divergent from v2.
        serde_json::json!({ "title": "f3", "command": "sh", "args": ["-c", "sleep 30"], "env": {} })
            .to_string()
            .as_bytes(),
    )
    .await;
    assert_eq!(status, 200, "terminal create: {created}");
    let pty_id = created["data"]["id"].as_str().expect("pty id").to_owned();

    let (status, socket) = ws_connect(
        server.addr,
        &format!("/api/experimental/persistent-pty/{pty_id}/connect"),
        None,
    )
    .await
    .expect("ws");
    assert_eq!(status, 101);
    let mut socket = socket.expect("upgraded");

    // attached → (replay) → replay_complete handshake.
    let mut saw_attached = false;
    let mut completed = false;
    while !completed {
        let frame = socket.recv(std::time::Duration::from_secs(10)).await.expect("event frame");
        if frame.opcode != 0x1 {
            continue;
        }
        let text = frame.text();
        if text.contains("\"attached\"") {
            saw_attached = true;
        }
        if text.contains("\"replay_complete\"") {
            completed = true;
        }
    }
    assert!(saw_attached, "attach handshake observed");
    assert!(completed, "replay_complete observed");

    // Resize through the production REST route.
    let (status, _, body) = raw_http(
        server.addr,
        "PUT",
        &format!("/api/experimental/persistent-pty/{pty_id}"),
        &[("content-type", "application/json")],
        serde_json::json!({ "size": { "cols": 100, "rows": 30 } }).to_string().as_bytes(),
    )
    .await;
    assert_eq!(status, 200, "resize: {body}");

    let frame = socket.recv(std::time::Duration::from_secs(10)).await.expect("resized frame");
    assert_eq!(frame.opcode, 0x1, "resized is a JSON text frame");
    let resized: Value = serde_json::from_str(&frame.text()).expect("resized JSON");
    assert_eq!(resized["type"], "resized");
    assert_eq!(resized["cols"], 100);
    assert_eq!(resized["rows"], 30);
    assert!(
        resized["checkpoint"].as_str().is_some_and(|checkpoint| !checkpoint.is_empty()),
        "resized carries a non-empty base64 checkpoint: {resized}"
    );
    assert!(resized["generation"].is_u64(), "resized carries the attachment generation: {resized}");
}
