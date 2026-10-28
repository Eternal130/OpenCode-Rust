//! Hand-written extension to the generated `event` namespace (kept out
//! of `methods.gen.rs` so regeneration never drops it): resubscribing
//! to `/api/event` with the SSE **`Last-Event-ID`** header.
//!
//! The header carries the id of the last event the client processed, so
//! a server that supports resumption replays (or continues from) that
//! cursor and one that does not simply ignores it — advisory per the
//! SSE spec, never an error. The TUI data layer
//! (`oc-tui::data::Connection`) wires its tracked cursor here on every
//! reconnect.

use std::sync::Arc;

use futures::StreamExt;

use crate::shared_events::{EventStream, SharedEvents};
use crate::transport::{Body, Call};

impl crate::methods::event::EventApi<'_> {
    /// GET /api/event with `Last-Event-ID: <cursor>` — the same call as
    /// the generated [`crate::methods::event::EventApi::subscribe`]
    /// plus the resume header (accepted or ignored by the server).
    pub async fn subscribe_from(
        &self,
        last_event_id: &str,
    ) -> Result<crate::sse::SseStream, crate::ClientError> {
        let headers: Vec<(&'static str, String)> =
            vec![("Last-Event-ID", last_event_id.to_owned())];
        self.client
            .execute_sse(
                &Call {
                    method: reqwest::Method::GET,
                    path: "/api/event",
                    path_params: &[],
                    query: &[],
                    headers: &headers,
                    body: Body::None,
                    success: 200,
                    declared: &[400, 401],
                },
                true,
            )
            .await
    }
}

impl crate::Client {
    /// Shared event stream over `/api/event` (v2 `SharedEvents.make`):
    /// refcounted subscribers share one SSE connection; the last dropped
    /// subscriber closes it; late subscribers receive only the latest
    /// `server.connected` marker; on stream end/error a fresh
    /// `subscribe()` opens a replacement connection (no auto-retry —
    /// the server is live-only, resync is snapshot-based per G-14).
    #[must_use]
    pub fn shared_events(&self) -> SharedEvents<serde_json::Value> {
        let client = self.clone();
        SharedEvents::new(Arc::new(move |activity| {
            let client = client.clone();
            Ok(connect_event_stream(client, activity))
        }))
    }
}

/// Opens `/api/event` and adapts [`crate::sse::SseStream`] into a boxed
/// event source, wiring `onActivity` (keepalive chunks) into the fan-out.
fn connect_event_stream(
    client: crate::Client,
    activity: crate::shared_events::Activity,
) -> EventStream<serde_json::Value> {
    Box::pin(
        futures::stream::once(async move {
            client
                .event()
                .subscribe()
                .await
                .map(|sse| sse.with_on_activity(move || activity.signal()).into_json_stream())
        })
        .flat_map(|result| match result {
            Ok(stream) => stream.left_stream(),
            Err(error) => futures::stream::once(async move { Err(error) }).right_stream(),
        }),
    )
}

#[cfg(test)]
mod tests {
    use crate::Client;

    /// The extension issues the same wire call as the generated
    /// subscribe (path + SSE double-decode), differing only by the
    /// resume header — pinned by a scripted server that echoes the
    /// header back in the first event's data.
    #[tokio::test]
    async fn subscribe_from_sends_the_last_event_id_header() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let (mut reader, _) = socket.split();
            let mut request = vec![0u8; 8192];
            let _ = reader.read(&mut request).await;
            let head = String::from_utf8_lossy(&request).to_lowercase();
            let seen = head
                .lines()
                .find(|line| line.starts_with("last-event-id:"))
                .map(str::to_owned)
                .unwrap_or_default();
            let body = format!("data: \"{{\\\"seen\\\":\\\"{seen}\\\"}}\"\n\n");
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n{body}"
            );
            let _ = socket.write_all(response.as_bytes()).await;
        });
        let client = Client::new(&format!("http://{addr}")).expect("client");
        let mut stream = client.event().subscribe_from("evt_42").await.expect("subscribe");
        let value = stream.next_json().await.expect("frame").expect("data");
        assert_eq!(value["seen"], "last-event-id: evt_42");
        server.abort();
    }
}

#[cfg(test)]
mod shared_tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use futures::StreamExt;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Encodes one `/api/event` data frame (JSON-in-string,
    /// double-decoded by `SseStream`).
    fn frame(event: &serde_json::Value) -> String {
        format!("data: {}\n\n", serde_json::to_string(event).unwrap())
    }

    /// Sequential fake `/api/event` server: one connection per accept,
    /// body scripted per connection, records request count, detects
    /// client-side close (EOF on the socket).
    async fn serve(
        listener: TcpListener,
        bodies: Vec<String>,
        requests: Arc<AtomicUsize>,
        client_closes: Arc<AtomicUsize>,
    ) {
        for body in bodies {
            let (mut socket, _) = listener.accept().await.expect("accept");
            requests.fetch_add(1, Ordering::SeqCst);
            let mut request = vec![0u8; 8192];
            let _ = socket.read(&mut request).await;
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n{body}"
            );
            let _ = socket.write_all(response.as_bytes()).await;
            // Hold the connection open; EOF means the client closed it.
            let mut scratch = [0u8; 64];
            if matches!(socket.read(&mut scratch).await, Ok(0)) {
                client_closes.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    /// End-to-end over HTTP: two subscribers share one connection, the
    /// last dropped subscriber closes it, a fresh subscribe opens a
    /// replacement connection, and no pre-reconnect event is replayed.
    #[tokio::test]
    async fn shared_events_share_and_reconnect_over_http() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let requests = Arc::new(AtomicUsize::new(0));
        let client_closes = Arc::new(AtomicUsize::new(0));
        let first_body = [
            frame(&serde_json::json!({"type": "server.connected", "value": 1})),
            frame(&serde_json::json!({"type": "session.updated", "value": 1})),
        ]
        .concat();
        let second_body = [
            frame(&serde_json::json!({"type": "server.connected", "value": 2})),
            frame(&serde_json::json!({"type": "session.updated", "value": 2})),
        ]
        .concat();
        {
            let requests = Arc::clone(&requests);
            let client_closes = Arc::clone(&client_closes);
            tokio::spawn(serve(listener, vec![first_body, second_body], requests, client_closes));
        }

        let client = crate::Client::new(&format!("http://{addr}")).expect("client");
        let shared = client.shared_events();
        let mut first = Box::pin(shared.subscribe());
        let mut second = Box::pin(shared.subscribe());
        let _ = futures::poll!(first.as_mut().next());
        let _ = futures::poll!(second.as_mut().next());

        assert_eq!(
            first.next().await.unwrap().unwrap(),
            serde_json::json!({"type": "server.connected", "value": 1})
        );
        assert_eq!(
            second.next().await.unwrap().unwrap(),
            serde_json::json!({"type": "server.connected", "value": 1})
        );
        assert_eq!(
            second.next().await.unwrap().unwrap(),
            serde_json::json!({"type": "session.updated", "value": 1})
        );
        assert_eq!(requests.load(Ordering::SeqCst), 1, "subscribers share one connection");

        // One subscriber dropping leaves the connection open.
        drop(first);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(client_closes.load(Ordering::SeqCst), 0);
        // The last subscriber out closes it (server sees EOF).
        drop(second);
        for _ in 0..200 {
            if client_closes.load(Ordering::SeqCst) == 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(
            client_closes.load(Ordering::SeqCst),
            1,
            "last subscriber closes the connection"
        );

        // Fresh subscribe reconnects (second HTTP request) and receives
        // only the replacement connection's events — no replay of the
        // pre-reconnect stream.
        let mut fresh = Box::pin(shared.subscribe());
        assert_eq!(
            fresh.next().await.unwrap().unwrap(),
            serde_json::json!({"type": "server.connected", "value": 2})
        );
        assert_eq!(
            fresh.next().await.unwrap().unwrap(),
            serde_json::json!({"type": "session.updated", "value": 2})
        );
        assert_eq!(
            requests.load(Ordering::SeqCst),
            2,
            "fresh subscribe opens a replacement connection"
        );
        drop(fresh);
    }
}
