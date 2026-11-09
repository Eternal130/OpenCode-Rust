//! G-197: middleware-declared 404 parity + per-request `RequestOptions`.
//!
//! * 404 parity: the `OpenAPI` export strips middleware error unions, so 45
//!   operations (every group wired with `locationMiddleware` in
//!   `packages/protocol/src/client.ts`) gained `LocationNotFoundError` 404
//!   via the generator overlay (`tools/openapi-gen/src/lib.rs`). A 404 from
//!   those ops must surface as the structured `ClientError::Api` — not
//!   `UnexpectedStatus`.
//! * `RequestOptions`: per-request headers (override client defaults),
//!   cancellation signal (including before send), and the SSE
//!   `onActivity` hook.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use oc_client::types::{AgentListQuery, ConfigGetQuery, FsListQuery};
use oc_client::{Client, ClientError, RequestOptions};

/// Reads one full HTTP request head (headers; these tests send no body).
fn read_request(stream: &mut TcpStream) -> std::io::Result<String> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let n = stream.read(&mut byte)?;
        if n == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "eof"));
        }
        buf.push(byte[0]);
        if buf.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&buf).to_string())
}

/// Spawns a mock server; `respond(conn_index, request, stream)` writes the
/// response. One request per connection.
fn spawn_mock<F>(respond: F) -> (String, Arc<Mutex<Vec<String>>>)
where
    F: Fn(usize, &str, &mut TcpStream) -> std::io::Result<()> + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let requests: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&requests);
    std::thread::spawn(move || {
        for (index, stream) in listener.incoming().enumerate() {
            let Ok(mut stream) = stream else { break };
            let Ok(request) = read_request(&mut stream) else { continue };
            recorded.lock().unwrap().push(request.clone());
            stream.set_write_timeout(Some(Duration::from_secs(5))).ok();
            let _ = respond(index, &request, &mut stream);
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    });
    (format!("http://{addr}"), requests)
}

fn json_response(status: u16, body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

fn location_404() -> Vec<u8> {
    json_response(404, r#"{"_tag":"LocationNotFoundError","message":"Location not found: /nope"}"#)
}

#[tokio::test]
async fn middleware_404_agent_list_is_structured_api_error() {
    let (url, _) = spawn_mock(|_, _, stream| stream.write_all(&location_404()));
    let c = Client::new(&url).expect("client");
    // `agent.list` had no 404 in the spec alone (middleware-declared only).
    let err = c.agent().list(&AgentListQuery::default()).await.expect_err("404");
    let ClientError::Api { status, name, message, .. } = &err else {
        panic!("expected Api error, got {err:?}")
    };
    assert_eq!(
        (*status, name.as_str(), message.as_str()),
        (404, "LocationNotFoundError", "Location not found: /nope")
    );
}

#[tokio::test]
async fn middleware_404_config_get_via_options_variant() {
    let (url, _) = spawn_mock(|_, _, stream| stream.write_all(&location_404()));
    let c = Client::new(&url).expect("client");
    // Same call through the `_with_options` tail-parameter variant.
    let err = c
        .config()
        .get_with_options(&ConfigGetQuery::default(), &RequestOptions::default())
        .await
        .expect_err("404");
    let ClientError::Api { status, name, .. } = &err else {
        panic!("expected Api error, got {err:?}")
    };
    assert_eq!((*status, name.as_str()), (404, "LocationNotFoundError"));
}

#[tokio::test]
async fn middleware_404_fs_list_alias_family() {
    // `fs.*` in the spec is the ref client's `file.*` namespace — the
    // middleware-404 overlay is keyed by operationId, so the alias family
    // is covered the same way.
    let (url, _) = spawn_mock(|_, _, stream| stream.write_all(&location_404()));
    let c = Client::new(&url).expect("client");
    let err = c.fs().list(&FsListQuery::default()).await.expect_err("404");
    let ClientError::Api { status, name, .. } = &err else {
        panic!("expected Api error, got {err:?}")
    };
    assert_eq!((*status, name.as_str()), (404, "LocationNotFoundError"));
}

#[tokio::test]
async fn undeclared_404_is_still_unexpected_status() {
    // `server.info` is NOT middleware-wired: a 404 stays UnexpectedStatus.
    let (url, _) = spawn_mock(|_, _, stream| stream.write_all(&location_404()));
    let c = Client::new(&url).expect("client");
    let err = c.server().info().await.expect_err("404");
    assert!(matches!(err, ClientError::UnexpectedStatus { status: 404, .. }), "{err:?}");
}

#[tokio::test]
async fn request_options_header_injection_and_override() {
    let (url, requests) = spawn_mock(|_, _, stream| {
        stream.write_all(&json_response(
            200,
            r#"{"version":"2.0.24","pid":1,"urls":["http://x"],"paths":{"tmp":"/tmp"}}"#,
        ))
    });
    let c = Client::new(&url)
        .expect("client")
        .with_default_header("x-client-default", "1")
        .expect("default header");
    let options = RequestOptions::new()
        .with_header("x-per-request", "yes")
        .expect("header")
        .with_header("x-client-default", "overridden")
        .expect("header");
    c.server().info_with_options(&options).await.expect("info");
    {
        // Scoped guard: holding it across the next `await` would deadlock
        // the mock's recorder.
        let recorded = requests.lock().unwrap();
        let request = &recorded[0];
        assert!(request.contains("x-per-request: yes"), "{request}");
        assert!(request.contains("x-client-default: overridden"), "{request}");
    }
    // Plain wrapper carries no per-request headers (default still present).
    c.server().info().await.expect("info");
    let recorded = requests.lock().unwrap();
    assert_eq!(recorded.len(), 2);
    assert!(recorded[1].contains("x-client-default: 1"), "{}", recorded[1]);
    assert!(!recorded[1].contains("x-per-request"), "{}", recorded[1]);
}

#[tokio::test]
async fn request_options_signal_cancels_before_send() {
    let (url, requests) = spawn_mock(|_, _, stream| {
        stream.write_all(&json_response(
            200,
            r#"{"version":"2.0.24","pid":1,"urls":["http://x"],"paths":{"tmp":"/tmp"}}"#,
        ))
    });
    let c = Client::new(&url).expect("client");
    let token = tokio_util::sync::CancellationToken::new();
    token.cancel();
    let err = c
        .server()
        .info_with_options(&RequestOptions::new().with_signal(token))
        .await
        .expect_err("cancelled");
    assert!(matches!(err, ClientError::Cancelled), "{err:?}");
    assert!(requests.lock().unwrap().is_empty(), "no request may reach the server");
}

#[tokio::test]
async fn request_options_on_activity_fires_per_sse_chunk() {
    let activity = Arc::new(AtomicUsize::new(0));
    let (url, _) = spawn_mock(|_, _, stream| {
        stream.write_all(
            b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n",
        )?;
        stream.write_all(b"data: 1\n\ndata: 2\n\n")
    });
    let c = Client::new(&url).expect("client");
    let mut stream = c
        .event()
        .subscribe_with_options(&RequestOptions::new().with_on_activity({
            let activity = Arc::clone(&activity);
            move || {
                activity.fetch_add(1, Ordering::SeqCst);
            }
        }))
        .await
        .expect("sse");
    assert_eq!(stream.next_frame().await.expect("f1").expect("some").data, "1");
    assert_eq!(stream.next_frame().await.expect("f2").expect("some").data, "2");
    assert!(activity.load(Ordering::SeqCst) >= 1, "onActivity must fire for received chunks");
}
