//! The C6b session-route scripted server (TUI-TD01/TD02/TD03/TD05/
//! TUI-TD07's PTY proofs): a self-contained HTTP+SSE stub the
//! `pty_session_route` and `pty_daily::queue_undo_refills` scenarios
//! drive the REAL app loop against — `pty_form`'s server shape extended
//! with paged `message.list`, parent-linked `session.list` and the
//! inbox mutation endpoints.
//!
//! Included as a module (`#[path = "../fixtures/route_server.rs"]`) —
//! files under `tests/fixtures/` are not standalone test targets.
#![allow(dead_code)] // each including binary uses a different subset

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// One recorded request (method, path-with-query, body).
#[derive(Debug, Clone)]
pub struct Recorded {
    pub method: String,
    pub path: String,
    pub body: String,
}

/// One `message.list` page: matched by the request's session + cursor
/// param (`None` cursor = the boot page).
#[derive(Clone)]
pub struct MessagePage {
    /// The session whose `message.list` this page answers (the path's
    /// `{sessionID}`).
    pub session: String,
    /// The cursor this page answers (`None` answers the cursorless
    /// boot fetch).
    pub cursor: Option<String>,
    /// The response body (already the `{"cursor":…,"data":[…]}` JSON).
    pub body: String,
}

pub struct ServerState {
    pub requests: Mutex<Vec<Recorded>>,
    pub sse: Mutex<Vec<Arc<Mutex<std::net::TcpStream>>>>,
    pub events: AtomicU64,
    /// The `session.list` rows (full JSON objects, joined by the
    /// handler).
    pub sessions: Vec<String>,
    /// The `message.list` pages (first match on the cursor param).
    pub pages: Vec<MessagePage>,
}

impl ServerState {
    fn record(&self, method: &str, path: &str, body: &str) {
        self.requests.lock().expect("requests").push(Recorded {
            method: method.to_owned(),
            path: path.to_owned(),
            body: body.to_owned(),
        });
    }

    /// The recorded requests whose path contains `needle`.
    pub fn requests_with(&self, needle: &str) -> Vec<Recorded> {
        self.requests
            .lock()
            .expect("requests")
            .iter()
            .filter(|request| request.path.contains(needle) || request.body.contains(needle))
            .cloned()
            .collect()
    }

    /// Push one bus event frame to every live SSE subscriber.
    #[allow(clippy::needless_pass_by_value)] // the builder ergonomics own the value
    pub fn push_event(&self, type_: &str, data: serde_json::Value) {
        let id = format!("evt_{}", self.events.fetch_add(1, Ordering::SeqCst));
        let event = serde_json::json!({ "id": id, "type": type_, "data": &data });
        let encoded = serde_json::to_string(&event).expect("event serializes");
        let frame = format!("data: {encoded}\n\n");
        let mut streams = self.sse.lock().expect("sse").clone();
        streams.retain(|stream| stream.lock().expect("stream").write_all(frame.as_bytes()).is_ok());
    }
}

/// The fake model catalog (one `parityfake/fake-model` entry).
pub fn model_catalog_json() -> String {
    let model = r#"{"providerID":"parityfake","modelID":"fake-model","id":"fake-model","name":"fake-model",
        "status":"active","enabled":true,"cost":[],"variants":[],
        "capabilities":{"input":["text"],"output":["text"],"tools":false},
        "limit":{"context":1000,"output":100},
        "time":{"released":1.0}}"#;
    format!(r#"{{"data":[{model}],"location":{{"directory":"/tmp"}}}}"#)
}

/// One `session.list` row (the parent link optional).
#[must_use]
pub fn session_json(id: &str, title: &str, parent: Option<&str>) -> String {
    let tokens =
        r#"{"cache":{"read":0,"write":0},"input":0,"output":0,"read":0,"write":0,"reasoning":0}"#;
    let parent = parent.map_or_else(String::new, |parent| format!(r#","parentID":"{parent}""#));
    format!(
        r#"{{"id":"{id}","title":"{title}"{parent},"time":{{"created":1,"updated":2,"idle":3,"viewed":3}},
            "location":{{"directory":"/tmp"}},"projectID":"p","cost":0,"tokens":{tokens}}}"#
    )
}

fn json_response(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn no_content() -> String {
    "HTTP/1.1 204 No Content\r\nconnection: close\r\n\r\n".to_owned()
}

#[allow(unused_assignments)]
#[allow(clippy::too_many_lines)]
fn handle(mut socket: std::net::TcpStream, state: &Arc<ServerState>) {
    socket.set_read_timeout(Some(Duration::from_secs(10))).ok();
    let mut buffer = [0u8; 16384];
    let mut head = String::new();
    let mut content_length = 0usize;
    loop {
        match socket.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(count) => {
                head.push_str(&String::from_utf8_lossy(&buffer[..count]));
                if let Some(rest) = head.split_once("\r\n\r\n") {
                    content_length = rest
                        .0
                        .lines()
                        .find_map(|line| {
                            let lower = line.to_ascii_lowercase();
                            lower
                                .strip_prefix("content-length:")
                                .and_then(|value| value.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if head.len() >= rest.0.len() + 4 + content_length {
                        break;
                    }
                }
            }
        }
    }
    let (head_text, body) = head.split_once("\r\n\r\n").expect("request head");
    let body = body.to_owned();
    socket.set_read_timeout(Some(Duration::from_millis(200))).ok();
    let mut parts = head_text.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let raw_path = parts.next().unwrap_or_default().to_owned();
    let path = raw_path.split('?').next().unwrap_or_default().to_owned();
    state.record(&method, &raw_path, &body);

    if path == "/api/event" {
        let response = b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\nconnection: close\r\n\r\n";
        if socket.write_all(response).is_err() {
            return;
        }
        let shared = Arc::new(Mutex::new(socket));
        state.sse.lock().expect("sse").push(Arc::clone(&shared));
        state.push_event("server.connected", serde_json::json!({}));
        {
            let stream = shared.lock().expect("stream");
            let _ = stream.set_read_timeout(Some(Duration::from_millis(25)));
        }
        let mut sink = [0u8; 256];
        loop {
            let result = shared.lock().expect("stream").read(&mut sink);
            match result {
                Ok(0) => break,
                Ok(_) => {}
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        || error.kind() == std::io::ErrorKind::TimedOut =>
                {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(_) => break,
            }
        }
        return;
    }

    let response = if path == "/api/model" {
        json_response(&model_catalog_json())
    } else if path == "/api/session" && method == "GET" {
        json_response(&format!(r#"{{"cursor":{{}},"data":[{}]}}"#, state.sessions.join(",")))
    } else if path.ends_with("/message") && method == "GET" {
        // The session + cursor params pick the page (`None` cursor =
        // the boot page).
        let session = path.trim_end_matches("/message").rsplit('/').next().unwrap_or_default();
        let cursor = raw_path.split('?').find_map(|query| {
            query.split('&').find_map(|pair| pair.strip_prefix("cursor=").map(str::to_owned))
        });
        let page = state
            .pages
            .iter()
            .find(|page| page.session == session && page.cursor.as_deref() == cursor.as_deref())
            .map_or_else(|| r#"{"cursor":{},"data":[]}"#.to_owned(), |page| page.body.clone());
        json_response(&page)
    } else if path.ends_with("/prompt") && method == "POST" {
        // session.prompt: the wire's data-wrapped inbox-user echo (the
        // client deserializes this — a bare `{}` fails and arms the
        // modal send-failure toast that swallows keys).
        let inbox_id = format!("inbox_{}", state.events.fetch_add(1, Ordering::SeqCst));
        let session_id = path.trim_end_matches("/prompt").rsplit('/').next().unwrap_or("s");
        json_response(&format!(
            r#"{{"data":{{"id":"{inbox_id}","delivery":"steer","type":"user","sessionID":"{session_id}","time":{{"created":1}},"payload":{{"text":""}}}}}}"#
        ))
    } else if path.ends_with("/model") && method == "POST" {
        // session.switchModel — 204 (the wire success shape).
        no_content()
    } else if path.contains("/inbox/") {
        // session.inbox.cancel (DELETE) / inbox.update (PATCH): the
        // TUI-TD07 mutation endpoints — record + 204.
        no_content()
    } else if path.ends_with("/permission") || path.contains("/form") {
        json_response(r#"{"data":[]}"#)
    } else {
        json_response("{}")
    };
    let _ = socket.write_all(response.as_bytes());
}

/// Spawn the scripted server on `127.0.0.1:0`.
pub fn spawn(state: ServerState) -> (String, Arc<ServerState>, Arc<AtomicBool>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let shutdown = Arc::new(AtomicBool::new(false));
    let state = Arc::new(state);
    let flag = Arc::clone(&shutdown);
    let server_state = Arc::clone(&state);
    std::thread::spawn(move || {
        listener.set_nonblocking(true).expect("nonblocking accept");
        while !flag.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((socket, _)) => {
                    let _ = socket.set_nodelay(true);
                    let state = Arc::clone(&server_state);
                    std::thread::spawn(move || handle(socket, &state));
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
    });
    (format!("http://{addr}"), state, shutdown)
}

/// A `message.list` page body over CHRONOLOGICAL user/assistant rows:
/// the wire carries `order: "desc"` (newest first — the fixture
/// reverses the input), and the client re-reverses to chronological.
#[must_use]
pub fn message_page_body(rows: &[String], next_cursor: Option<&str>) -> String {
    let cursor = match next_cursor {
        Some(next) => format!(r#"{{"next":"{next}","previous":null}}"#),
        None => "{}".to_owned(),
    };
    let mut wire = rows.to_vec();
    wire.reverse();
    format!(r#"{{"cursor":{cursor},"data":[{}]}}"#, wire.join(","))
}

/// One user-message row.
#[must_use]
pub fn user_row(id: &str, text: &str) -> String {
    format!(r#"{{"type":"user","id":"{id}","text":{text},"time":{{"created":1}}}}"#)
}

/// One assistant-message row with a single text part (the text is a
/// JSON string literal — multi-line bodies welcome).
#[must_use]
pub fn assistant_row(id: &str, text: &str) -> String {
    format!(
        r#"{{"type":"assistant","id":"{id}","agent":"build","model":{{"providerID":"p","id":"m"}},
            "time":{{"created":2,"completed":3}},
            "content":[{{"type":"text","text":{text},"time":{{"created":2,"completed":3}}}}],
            "tokens":{{"cache":{{"read":0,"write":0}},"input":1,"output":1,"read":0,"write":0,"reasoning":0}},
            "cost":0}}"#
    )
}

/// JSON-encode one string literal.
#[must_use]
pub fn json_string(text: &str) -> String {
    serde_json::to_string(text).expect("encodes")
}
