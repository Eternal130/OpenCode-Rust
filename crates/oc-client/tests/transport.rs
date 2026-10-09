//! Transport-level tests against a hand-rolled `std::net::TcpListener` HTTP
//! responder
//! (no axum/hyper in the dev-deps): Basic auth injection, query/path/query-
//! struct rendering, error envelopes (declared + `UnexpectedStatus`), 204s,
//! binary endpoints, and SSE decoding incl. CRLF, heartbeats, multi-line
//! data, the 16 MiB cap and the content-type gate.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;

use oc_client::types::{
    AgentListQuery, PtyConnectTokenQuery, QueryLocation,
    ServerExperimentalPersistentPtyConnectTokenQuery, SessionListQuery, SessionListQueryOrder,
    SessionMessageListQuery, SessionMessageListQueryOrder, SessionMessageListQueryType,
};
use oc_client::{Client, ClientError};

struct Mock {
    addr: std::net::SocketAddr,
    requests: Arc<Mutex<Vec<String>>>,
    _handle: std::thread::JoinHandle<()>,
}

impl Mock {
    fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

/// Reads one full HTTP request (headers + content-length body).
fn read_request(stream: &mut TcpStream) -> std::io::Result<String> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let n = stream.read(&mut byte)?;
        if n == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "eof in headers"));
        }
        buf.push(byte[0]);
        if buf.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let head = String::from_utf8_lossy(&buf).to_string();
    let length = head
        .lines()
        .find_map(|l| {
            let (name, value) = l.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())?
        })
        .unwrap_or(0);
    let mut body = vec![0u8; length];
    stream.read_exact(&mut body)?;
    Ok(format!("{head}{}", String::from_utf8_lossy(&body)))
}

/// Spawns a mock server; `respond(conn_index, request, stream)` writes the
/// response and returns. One request per connection.
fn spawn_mock<F>(respond: F) -> Mock
where
    F: Fn(usize, &str, &mut TcpStream) -> std::io::Result<()> + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let requests: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::new(AtomicUsize::new(0));
    let handle = {
        let requests = Arc::clone(&requests);
        let seen = Arc::clone(&seen);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let Ok(request) = read_request(&mut stream) else { continue };
                let index = seen.fetch_add(1, Ordering::SeqCst);
                requests.lock().unwrap().push(request.clone());
                stream.set_write_timeout(Some(Duration::from_secs(5))).ok();
                let _ = respond(index, &request, &mut stream);
                let _ = stream.shutdown(std::net::Shutdown::Both);
                if Arc::clone(&seen).load(Ordering::SeqCst) >= 1 && index >= 100 {
                    break;
                }
            }
        })
    };
    Mock { addr, requests, _handle: handle }
}

fn json_response(status: u16, body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

fn client(url: &str) -> Client {
    Client::new(url).expect("client")
}

#[tokio::test]
async fn basic_auth_header_injected() {
    let mock = spawn_mock(|_, _, stream| {
        stream.write_all(&json_response(
            200,
            r#"{"version":"2.0.24","pid":1,"urls":["http://x"],"paths":{"tmp":"/tmp"}}"#,
        ))
    });
    let c = client(&mock.url()).with_basic_auth("opencode", Some("password".to_owned()));
    let info = c.server().info().await.expect("info");
    assert_eq!(info.version, "2.0.24");
    let request = &mock.requests()[0];
    assert!(
        request.to_ascii_lowercase().contains("authorization: basic b3blbmnvzgu6cgfzc3dvcmq="),
        "Basic header missing: {request}"
    );
    assert!(request.starts_with("GET /api/info HTTP/1.1"));
}

#[tokio::test]
async fn no_auth_header_and_default_headers_merged() {
    let mock = spawn_mock(|_, _, stream| {
        stream.write_all(&json_response(
            200,
            r#"[{"name":"bash","acceptable":true,"path":"/bin/bash"}]"#,
        ))
    });
    let c = client(&mock.url()).with_default_header("x-custom", "1").expect("header");
    let shells = c.config().shells().await.expect("shells");
    assert_eq!(shells[0].name, "bash");
    let request = &mock.requests()[0];
    assert!(!request.to_ascii_lowercase().contains("authorization:"));
    assert!(request.to_ascii_lowercase().contains("x-custom: 1"));
}

#[tokio::test]
async fn query_params_path_encoding_and_enums() {
    let mock = spawn_mock(|_, _, stream| {
        stream
            .write_all(&json_response(200, r#"{"data":[],"cursor":{"previous":null,"next":null}}"#))
    });
    let c = client(&mock.url());
    let query = SessionMessageListQuery {
        cursor: Some("abc".to_owned()),
        limit: Some("5".to_owned()),
        order: Some(SessionMessageListQueryOrder::Desc),
        type_: Some(SessionMessageListQueryType::User),
    };
    c.message().list("ses 1/x", &query).await.expect("message list");
    let request = &mock.requests()[0];
    assert!(
        request.starts_with("GET /api/session/ses%201%2Fx/message?"),
        "path not percent-encoded: {request}"
    );
    assert!(request.contains("cursor=abc"), "{request}");
    assert!(request.contains("limit=5"), "{request}");
    assert!(request.contains("order=desc"), "{request}");
    assert!(request.contains("type=user"), "{request}");
}

#[tokio::test]
async fn location_directory_query_expansion() {
    let mock = spawn_mock(|_, _, stream| {
        stream.write_all(&json_response(200, r#"{"location":{"directory":"/w"},"data":[]}"#))
    });
    let c = client(&mock.url());
    let query =
        AgentListQuery { location: Some(QueryLocation { directory: Some("/work".to_owned()) }) };
    let response = c.agent().list(&query).await.expect("agent list");
    // {location, data} envelopes are kept whole (v2 parity).
    assert_eq!(response.location.directory, "/w");
    assert_eq!(response.data.len(), 0);
    let request = &mock.requests()[0];
    assert!(
        request.contains("location%5Bdirectory%5D=%2Fwork"),
        "location[directory] not expanded: {request}"
    );
}

#[tokio::test]
async fn data_envelope_unwrapped() {
    let mock = spawn_mock(|_, _, stream| {
        stream.write_all(&json_response(200, r#"{"data":{"ticket":"t1","expires_in":60}}"#))
    });
    let c = client(&mock.url());
    let token = c
        .persistent_pty()
        .connect_token(
            "pty_1",
            &ServerExperimentalPersistentPtyConnectTokenQuery {
                x_opencode_ticket: Some("1".to_owned()),
            },
        )
        .await
        .expect("token");
    assert_eq!(token.ticket, "t1");
    assert_eq!(token.expires_in, 60);
    let request = &mock.requests()[0];
    assert!(request.contains("x-opencode-ticket: 1"), "{request}");
}

#[tokio::test]
async fn pty_connect_token_query_struct_header() {
    // distinct header param surface for the non-experimental variant
    let mock = spawn_mock(|_, _, stream| {
        stream.write_all(&json_response(
            200,
            r#"{"location":{"directory":"/w"},"data":{"ticket":"t","expires_in":5}}"#,
        ))
    });
    let c = client(&mock.url());
    let query = PtyConnectTokenQuery {
        location: Some(QueryLocation { directory: Some("/w".to_owned()) }),
        x_opencode_ticket: Some("1".to_owned()),
    };
    let token = c.pty().connect_token("pty_9", &query).await.expect("token");
    assert_eq!(token.data.expires_in, 5);
    assert!(mock.requests()[0].contains("x-opencode-ticket: 1"));
}

#[tokio::test]
async fn declared_error_envelope_with_fields() {
    let mock = spawn_mock(|_, _, stream| {
        stream.write_all(&json_response(
            404,
            r#"{"_tag":"SessionNotFoundError","sessionID":"ses_1","message":"Session not found"}"#,
        ))
    });
    let c = client(&mock.url());
    let err = c.session().get("ses_1").await.expect_err("declared error");
    let ClientError::Api { status, name, message, fields } = err else {
        panic!("expected Api error, got {err:?}")
    };
    assert_eq!(status, 404);
    assert_eq!(name, "SessionNotFoundError");
    assert_eq!(message, "Session not found");
    assert_eq!(fields.get("sessionID"), Some(&json!("ses_1")));
    assert_eq!(fields.get("_tag"), Some(&json!("SessionNotFoundError")));
}

#[tokio::test]
async fn declared_error_message_falls_back_to_data_message() {
    let mock = spawn_mock(|_, _, stream| {
        stream.write_all(&json_response(
            500,
            r#"{"_tag":"RpcInternalError","data":{"message":"boom"}}"#,
        ))
    });
    let c = client(&mock.url());
    let err = c
        .rpc()
        .call(
            "rpcID",
            "method",
            &oc_client::types::RpcCallQuery { location: None },
            &oc_client::types::RpcInput { input: Some(json!({"a": 1})) },
        )
        .await
        .expect_err("declared");
    let ClientError::Api { name, message, .. } = err else {
        panic!("expected Api error, got {err:?}")
    };
    assert_eq!(name, "RpcInternalError");
    assert_eq!(message, "boom");
}

#[tokio::test]
async fn undeclared_status_is_unexpected() {
    let mock = spawn_mock(|_, _, stream| stream.write_all(&json_response(418, "teapot")));
    let c = client(&mock.url());
    let err = c.server().info().await.expect_err("undeclared");
    assert!(
        matches!(&err, ClientError::UnexpectedStatus { status: 418, body } if body == "teapot"),
        "got {err:?}"
    );
}

#[tokio::test]
async fn declared_error_with_non_json_body() {
    let mock = spawn_mock(|_, _, stream| {
        stream.write_all(
            b"HTTP/1.1 400 X\r\ncontent-type: text/plain\r\ncontent-length: 3\r\nconnection: close\r\n\r\nbad",
        )
    });
    let c = client(&mock.url());
    let err = c.server().info().await.expect_err("non-json error");
    assert!(matches!(err, ClientError::UnsupportedContentType(_)), "{err:?}");
}

#[tokio::test]
async fn success_with_wrong_content_type_and_malformed_body() {
    let mock = spawn_mock(|_, _, stream| {
        stream.write_all(
            b"HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\ncontent-length: 2\r\nconnection: close\r\n\r\nhi",
        )
    });
    let err = client(&mock.url()).server().info().await.expect_err("ct");
    assert!(matches!(err, ClientError::UnsupportedContentType(_)), "{err:?}");

    let mock = spawn_mock(|_, _, stream| stream.write_all(&json_response(200, "not-json{")));
    let err = client(&mock.url()).server().info().await.expect_err("malformed");
    assert!(matches!(err, ClientError::MalformedResponse(_)), "{err:?}");
}

#[tokio::test]
async fn no_content_204_returns_unit() {
    let mock = spawn_mock(|_, _, stream| {
        stream
            .write_all(b"HTTP/1.1 204 No Content\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
    });
    let c = client(&mock.url());
    c.session().remove("ses_1").await.expect("204 -> ()");
}

#[tokio::test]
async fn binary_request_and_response() {
    let mock = spawn_mock(|index, request, stream| {
        if index == 0 {
            // fs.write -> { location, data: FileSystem.Write }
            assert!(request.contains("content-type: application/octet-stream"));
            assert!(request.ends_with("file-bytes"));
            stream.write_all(&json_response(
                200,
                r#"{"location":{"directory":"/w"},"data":{"path":"/tmp/x"}}"#,
            ))
        } else {
            stream.write_all(
                b"HTTP/1.1 200 OK\r\ncontent-type: application/octet-stream\r\nconnection: close\r\n\r\nraw-\xc3\xa9-bytes",
            )
        }
    });
    let c = client(&mock.url());
    let query =
        oc_client::types::ExperimentalFsWriteQuery { location: None, path: "/tmp/x".to_owned() };
    c.fs().write(&query, b"file-bytes").await.expect("fs write");
    let read = c
        .fs()
        .read(
            "dir/sub/file.txt",
            &oc_client::types::FsReadQuery {
                location: Some(QueryLocation { directory: Some("/w".to_owned()) }),
            },
        )
        .await
        .expect("fs read");
    assert_eq!(read, "raw-é-bytes".as_bytes());
    let request = &mock.requests()[1];
    assert!(
        request.starts_with("GET /api/fs/read/dir/sub/file.txt?"),
        "wildcard path must keep slashes: {request}"
    );
}

#[tokio::test]
async fn sse_decoding_multi_data_heartbeats_and_crlf() {
    let mock = spawn_mock(|_, _, stream| {
        stream.write_all(
            b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n",
        )?;
        stream.write_all(b"data: {\"a\":\r\n")?;
        std::thread::sleep(Duration::from_millis(30));
        stream.write_all(b"data: 1}\r\n\r\n")?;
        stream.write_all(b": heartbeat\n\n")?;
        stream.write_all(b"id: 7\nevent: message.updated\ndata: 42\n\n")?;
        std::thread::sleep(Duration::from_millis(30));
        Ok(())
    });
    let c = client(&mock.url());
    let activity = Arc::new(AtomicUsize::new(0));
    let mut stream = c.event().subscribe().await.expect("sse").with_on_activity({
        let activity = Arc::clone(&activity);
        move || {
            activity.fetch_add(1, Ordering::SeqCst);
        }
    });
    let first = stream.next_frame().await.expect("frame 1").expect("some");
    assert_eq!(first.data, "{\"a\":\n1}");
    let second = stream.next_frame().await.expect("frame 2").expect("some");
    assert_eq!(second.data, "42");
    assert_eq!(second.id.as_deref(), Some("7"));
    assert_eq!(second.event.as_deref(), Some("message.updated"));
    let third = stream.next_frame().await.expect("eof");
    assert!(third.is_none());
    assert!(activity.load(Ordering::SeqCst) >= 2, "onActivity per chunk");
}

#[tokio::test]
async fn sse_double_decoded_events() {
    let mock = spawn_mock(|_, _, stream| {
        let payload = serde_json::to_string(&json!({"type":"server.connected","data":{}})).unwrap();
        let inner = serde_json::to_string(&payload).unwrap();
        let frame = format!("data: {inner}\n\n");
        stream.write_all(
            b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n",
        )?;
        stream.write_all(frame.as_bytes())
    });
    let c = client(&mock.url());
    let mut stream = c.event().subscribe().await.expect("sse");
    assert!(stream.double_decode());
    let value = stream.next_json().await.expect("json").expect("some");
    assert_eq!(value, json!({"type":"server.connected","data":{}}));
}

#[tokio::test]
async fn sse_content_type_gate() {
    let mock = spawn_mock(|_, _, stream| stream.write_all(&json_response(200, r#"{"not":"sse"}"#)));
    let err = match client(&mock.url()).event().subscribe().await {
        Ok(_stream) => panic!("expected UnsupportedContentType"),
        Err(e) => e,
    };
    assert!(matches!(err, ClientError::UnsupportedContentType(_)), "{err:?}");
}

#[tokio::test]
async fn sse_error_status_classified() {
    let mock = spawn_mock(|_, _, stream| {
        stream.write_all(&json_response(404, r#"{"_tag":"SessionNotFoundError","message":"no"}"#))
    });
    let err = match client(&mock.url())
        .session()
        .log("ses_1", &oc_client::types::SessionLogQuery { after: None, follow: None })
        .await
    {
        Ok(_stream) => panic!("expected declared error"),
        Err(e) => e,
    };
    assert_eq!(err.api_name(), Some("SessionNotFoundError"));
}

#[tokio::test]
async fn sse_buffer_cap() {
    // The default cap is the v2 16 MiB constant; the mechanism is proven
    // here with a small override.
    assert_eq!(oc_client::MAX_SSE_EVENT_BYTES, 16 * 1024 * 1024);
    let mock = spawn_mock(|_, _, stream| {
        stream.write_all(
            b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\ndata: ",
        )?;
        let big = vec![b'x'; 512];
        stream.write_all(&big) // never terminated -> keeps buffering
    });
    let c = client(&mock.url());
    let mut stream = c.event().subscribe().await.expect("sse").with_max_buffer(64);
    let err = stream.next_frame().await.expect_err("too large");
    assert!(
        matches!(err, ClientError::SseEventTooLarge(64)),
        "expected SseEventTooLarge, got {err:?}"
    );
}

#[tokio::test]
async fn sse_malformed_json_is_malformed_response() {
    let mock = spawn_mock(|_, _, stream| {
        stream.write_all(
            b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\ndata: not-json\n\n",
        )
    });
    let c = client(&mock.url());
    let mut stream = c.event().subscribe().await.expect("sse");
    let err = stream.next_json().await.expect_err("malformed");
    assert!(matches!(err, ClientError::MalformedResponse(_)), "{err:?}");
}

#[tokio::test]
async fn auth_token_url_helper() {
    let c = client("http://127.0.0.1:1")
        .with_basic_auth("opencode", Some("pw".to_owned()))
        .with_auth_token("QUJD".to_owned());
    assert_eq!(
        c.url_with_auth_token("/api/pty/x/connect?cursor=0"),
        Some("http://127.0.0.1:1/api/pty/x/connect?cursor=0&auth_token=QUJD".to_owned())
    );
    assert_eq!(
        c.url_with_auth_token("/ws"),
        Some("http://127.0.0.1:1/ws?auth_token=QUJD".to_owned())
    );
    // Without any credentials there is nothing to append.
    assert_eq!(client("http://127.0.0.1:1").url_with_auth_token("/ws"), None);
}

#[tokio::test]
async fn session_list_cursor_query_roundtrip() {
    let mock = spawn_mock(|_, _, stream| {
        stream.write_all(&json_response(
            200,
            r#"{"data":[],"cursor":{"previous":null,"next":"nxt"}}"#,
        ))
    });
    let c = client(&mock.url());
    let query = SessionListQuery {
        cursor: Some("cur".to_owned()),
        directory: Some("/d".to_owned()),
        limit: Some("2".to_owned()),
        order: Some(SessionListQueryOrder::Asc),
        parent_id: Some("null".to_owned()),
        project: None,
        search: Some("term with space".to_owned()),
        subpath: None,
    };
    let response = c.session().list(&query).await.expect("list");
    assert_eq!(response.cursor.next.as_deref(), Some("nxt"));
    let request = &mock.requests()[0];
    assert!(request.contains("search=term+with+space"), "{request}");
    assert!(request.contains("parentID=null"), "{request}");
}
