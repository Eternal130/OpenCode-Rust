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
