//! Typed `rpc.call` wrapper (RE-06): the generated
//! `RpcApi::call` is a single generic method whose `RpcInput`/`RpcOutput`
//! cores are `Option<serde_json::Value>` — wire drift decodes to silent
//! `None`s. This module mirrors the ref's schema-driven runtime
//! (`packages/client/src/rpc-runtime.ts` `read`): a typed request/response
//! boundary that ERRORS loudly when the response shape does not match
//! `O` (use `#[serde(deny_unknown_fields)]` on `O` where the ref method's
//! output schema is closed, so unknown fields surface too).
//!
//! Kept hand-written (outside `methods.gen.rs`) so regeneration never
//! drops it, and bounded to a generic wrapper rather than codegen of
//! all 141 RPC method definitions.
//!
//! # Typed per-definition surface (G-122)
//!
//! The ref's `makeRpc` (promise/rpc.ts:53-145) has NO fixed method
//! list: it generates one callable per key of a runtime
//! `Rpc.PortableDefinition` (schema/rpc.ts) — the "141 methods" are one
//! plugin's definition, not a schema inventory. The faithful port is
//! therefore definition-driven, not a static 141-function table:
//! [`RpcDefinition`] carries the portable definition (method/event
//! names plus their opaque JSON Schemas) and [`RpcClient`] exposes the
//! same three faces `makeRpc` returns — typed `call` per method,
//! `events.subscribe(name)` filtered on the wire type
//! `rpc.<id>.<event>` (schema/rpc.ts `eventType`), and
//! `events.on(name, handler)` with a drop-to-unsubscribe guard.
//! Bounded deviation from TS: methods dispatch through
//! `RpcClient::call(name, ..)` validated against the definition's
//! method table (loud `Unknown RPC method` error, mirroring the
//! server-side `rpc.method_not_found`) instead of one generated fn per
//! name — the membership check is the same runtime check the TS proxy
//! performs; Rust has no runtime object-surface generation (G-122).

use std::collections::BTreeMap;
use std::pin::Pin;
use std::sync::Arc;

use futures::Stream;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::bus_event::BusEventFrame;
use crate::error::ClientError;

/// The filtered event stream returned by [`RpcClient::subscribe_events`].
type FilteredBusEvents = Pin<Box<dyn Stream<Item = Result<BusEventFrame, ClientError>> + Send>>;

impl crate::methods::rpc::RpcApi<'_> {
    /// `rpc.call` with typed input and output: serializes `input` into
    /// the `RpcInput` envelope, calls the generic method, then decodes
    /// the response's `output` core into `O`, mapping every failure to
    /// a loud [`ClientError::MalformedResponse`] that names the method:
    ///
    /// * `output` absent or `null` → error (a silent-`None` decode is
    ///   impossible by construction);
    /// * `output` not matching `O` (missing/renamed fields; unknown
    ///   fields when `O` denies them) → error carrying the serde
    ///   message.
    pub async fn call_into<I, O>(
        &self,
        rpc_id: &str,
        method: &str,
        query: &crate::types::RpcCallQuery,
        input: &I,
    ) -> Result<O, ClientError>
    where
        I: Serialize,
        O: DeserializeOwned,
    {
        let input_value =
            serde_json::to_value(input).map_err(|e| ClientError::Encoding(e.to_string()))?;
        let response = self
            .call(rpc_id, method, query, &crate::types::RpcInput { input: Some(input_value) })
            .await?;
        let output = response.output.ok_or_else(|| {
            ClientError::MalformedResponse(format!(
                "rpc.call `{method}` response missing `output` — wire drift"
            ))
        })?;
        serde_json::from_value(output).map_err(|error| {
            ClientError::MalformedResponse(format!(
                "rpc.call `{method}` output decode failed: {error}"
            ))
        })
    }
}

// ---------------------------------------------------------------------------
// G-122: per-definition typed RPC surface (promise/rpc.ts `makeRpc`)
// ---------------------------------------------------------------------------

/// One portable RPC definition (port of `Rpc.PortableDefinition`,
/// schema/rpc.ts:41-53): an `id`, a method table, and an event table.
/// The input/output/error schemas are carried opaque (JSON Schema
/// `Value`s) exactly like the ref's portable form — wire decode stays
/// loud via [`RpcClient::call`] /
/// [`crate::methods::rpc::RpcApi::call_into`].
#[derive(Debug, Clone)]
pub struct RpcDefinition {
    pub id: String,
    /// `definition.methods` — name → { input, output, errors? }.
    pub methods: BTreeMap<String, RpcMethodDef>,
    /// `definition.events` — name → { schema }.
    pub events: BTreeMap<String, serde_json::Value>,
}

/// One method's portable schema (`Rpc.PortableMethod`).
#[derive(Debug, Clone)]
pub struct RpcMethodDef {
    /// JSON Schema for the input (opaque).
    pub input: serde_json::Value,
    /// JSON Schema for the output (opaque).
    pub output: serde_json::Value,
    /// Declared error schemas, name → schema (`errors?`).
    pub errors: BTreeMap<String, serde_json::Value>,
}

impl RpcDefinition {
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into(), methods: BTreeMap::new(), events: BTreeMap::new() }
    }

    /// Registers a method (`definition.methods[name]`).
    #[must_use]
    pub fn method(
        mut self,
        name: impl Into<String>,
        input: serde_json::Value,
        output: serde_json::Value,
    ) -> Self {
        self.methods.insert(name.into(), RpcMethodDef { input, output, errors: BTreeMap::new() });
        self
    }

    /// Registers an event (`definition.events[name]`).
    #[must_use]
    pub fn event(mut self, name: impl Into<String>, schema: serde_json::Value) -> Self {
        self.events.insert(name.into(), schema);
        self
    }

    /// The wire event type for event `name` (`rpc.<id>.<name>`,
    /// promise/rpc.ts:145-147 `eventType`) — `None` for events the
    /// definition does not declare.
    #[must_use]
    pub fn event_type(&self, name: &str) -> Option<String> {
        self.events.contains_key(name).then(|| format!("rpc.{}.{}", self.id, name))
    }

    /// Binds this definition to a client (`makeRpc(raw, events)`).
    #[must_use]
    pub fn client(self, client: crate::Client) -> RpcClient {
        RpcClient { client, definition: Arc::new(self) }
    }
}

/// The typed per-definition RPC client (port of `makeRpc`'s return: one
/// callable per method + an `events` namespace with `subscribe`/`on`).
pub struct RpcClient {
    client: crate::Client,
    definition: Arc<RpcDefinition>,
}

impl RpcClient {
    /// The bound definition.
    #[must_use]
    pub fn definition(&self) -> &RpcDefinition {
        &self.definition
    }

    /// `rpc.call` typed through the definition's method table: unknown
    /// method names fail loudly BEFORE any request (the ref's generated
    /// functions simply do not exist for unknown names; the server would
    /// answer `rpc.method_not_found`).
    pub async fn call<I, O>(
        &self,
        method: &str,
        location: Option<&crate::types::QueryLocation>,
        input: &I,
    ) -> Result<O, ClientError>
    where
        I: Serialize,
        O: DeserializeOwned,
    {
        if !self.definition.methods.contains_key(method) {
            return Err(ClientError::MalformedResponse(format!(
                "Unknown RPC method: {}.{} (defined: {:?})",
                self.definition.id,
                method,
                self.definition.methods.keys().collect::<Vec<_>>()
            )));
        }
        self.client
            .rpc()
            .call_into(
                &self.definition.id,
                method,
                &crate::types::RpcCallQuery { location: location.cloned() },
                input,
            )
            .await
    }

    /// The validated `rpc.<id>.<name>` event type (the ref's `eventType`
    /// lookup; errors on unknown event names).
    pub fn event_type(&self, name: &str) -> Result<String, ClientError> {
        self.definition.event_type(name).ok_or_else(|| {
            ClientError::MalformedResponse(format!(
                "Unknown RPC event: {}.{}",
                self.definition.id, name
            ))
        })
    }

    /// `events.subscribe(name)` (promise/rpc.ts:57-90): joins the
    /// client-wide SHARED `/api/event` stream (G-376① — the ref's
    /// `makeRpc(raw, events)` receives the client's single
    /// `SharedEvents` instance from `make()`, so every `on()`/
    /// `subscribe()` fans out over one connection) and filters to the
    /// definition's `rpc.<id>.<name>` wire type. Unknown event names
    /// error loudly (`Unknown RPC event: <id>.<name>`), exactly like
    /// the ref.
    pub fn subscribe_events(&self, name: &str) -> Result<FilteredBusEvents, ClientError> {
        let type_ = self.event_type(name)?;
        let shared = self.client.shared_bus_events();
        Ok(filter_rpc_events(shared.subscribe(), type_))
    }

    /// `events.on(name, handler)` (promise/rpc.ts:95-110): spawns a task
    /// that forwards matching events to `handler` until the returned
    /// guard is dropped (the ref's unsubscribe aborts the controller).
    /// Source errors terminate the loop; a fresh `on`/`subscribe_events`
    /// reopens the connection (no auto-retry, G-14).
    pub fn on<F, Fut>(&self, name: &str, mut handler: F) -> Result<RpcEventGuard, ClientError>
    where
        F: FnMut(BusEventFrame) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send,
    {
        let mut events = self.subscribe_events(name)?;
        let task = tokio::spawn(async move {
            use futures::StreamExt;
            while let Some(item) = events.next().await {
                match item {
                    Ok(frame) => handler(frame).await,
                    Err(_) => break,
                }
            }
        });
        Ok(RpcEventGuard { task: Some(task) })
    }
}

/// Drop-to-unsubscribe handle for [`RpcClient::on`] (the ref's returned
/// `() => controller.abort()` closure).
pub struct RpcEventGuard {
    task: Option<tokio::task::JoinHandle<()>>,
}

impl RpcEventGuard {
    /// Explicitly unsubscribes (equivalent to dropping the guard).
    pub fn unsubscribe(mut self) {
        self.abort();
    }

    fn abort(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

impl Drop for RpcEventGuard {
    fn drop(&mut self) {
        self.abort();
    }
}

/// The `rpc.<id>.<event>` filter (promise/rpc.ts:63-65): only frames
/// whose wire `type` equals `type_` pass; everything else is skipped,
/// and the filtered stream ends when the source ends.
pub fn filter_rpc_events<S>(source: S, type_: String) -> FilteredBusEvents
where
    S: Stream<Item = Result<BusEventFrame, ClientError>> + Send + 'static,
{
    use futures::StreamExt;
    Box::pin(source.filter_map(move |item| {
        futures::future::ready(match item {
            Ok(frame) if frame.type_ == type_ => Some(Ok(frame)),
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        })
    }))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde::Deserialize;
    use serde_json::json;

    use crate::Client;

    /// Minimal scripted HTTP server answering one request.
    async fn serve_once(status: u16, body: &'static str) -> (Client, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let task = tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut request = vec![0u8; 8192];
            let _ = socket.read(&mut request).await;
            let response = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
        });
        (Client::new(&format!("http://{addr}")).expect("client"), task)
    }

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct EchoOutput {
        value: u32,
    }

    /// G-122: a definition-driven client validates method/event names
    /// against the definition table — the runtime equivalent of the
    /// ref's generated per-name functions.
    fn demo_definition() -> crate::rpc_typed::RpcDefinition {
        crate::rpc_typed::RpcDefinition::new("demo")
            .method("echo", json!({}), json!({}))
            .method("ping", json!({}), json!({}))
            .event("updated", json!({}))
            .event("deleted", json!({}))
    }

    #[tokio::test]
    async fn unknown_method_fails_before_any_request() {
        let client = Client::new("http://127.0.0.1:1").expect("client");
        let rpc = demo_definition().client(client);
        let error = rpc
            .call::<serde_json::Value, serde_json::Value>("nope", None, &json!({}))
            .await
            .expect_err("unknown method must fail");
        assert!(error.to_string().contains("Unknown RPC method: demo.nope"), "{error}");
    }

    #[tokio::test]
    async fn known_method_dispatches_through_rpc_call() {
        let (client, server) = serve_once(200, r#"{"output":{"value":9}}"#).await;
        let rpc = demo_definition().client(client);
        let output: EchoOutput = rpc.call("echo", None, &json!({"a": 1})).await.expect("dispatch");
        assert_eq!(output.value, 9);
        server.abort();
    }

    /// `eventType` lookup: known events map to `rpc.<id>.<name>`,
    /// unknown ones error loudly (promise/rpc.ts:59,146).
    #[test]
    fn event_type_lookup_and_unknown_event_error() {
        let client = Client::new("http://127.0.0.1:1").expect("client");
        let rpc = demo_definition().client(client);
        assert_eq!(rpc.event_type("updated").unwrap(), "rpc.demo.updated");
        let error = rpc.event_type("nope").expect_err("unknown event must fail");
        assert!(error.to_string().contains("Unknown RPC event: demo.nope"), "{error}");
    }

    /// The event filter passes only the `rpc.<id>.<event>` frames.
    #[tokio::test]
    async fn filter_rpc_events_selects_only_the_definition_event() {
        use futures::StreamExt;
        let source = futures::stream::iter([
            Ok(crate::bus_event::BusEventFrame::decode(&json!({
                "type": "server.connected"
            }))
            .unwrap()),
            Ok(crate::bus_event::BusEventFrame::decode(&json!({
                "type": "rpc.other.happened", "data": {}
            }))
            .unwrap()),
            Ok(crate::bus_event::BusEventFrame::decode(&json!({
                "type": "rpc.demo.updated", "data": {"n": 1}
            }))
            .unwrap()),
            Ok(crate::bus_event::BusEventFrame::decode(&json!({
                "type": "rpc.demo.deleted", "data": {}
            }))
            .unwrap()),
        ]);
        let mut filtered =
            crate::rpc_typed::filter_rpc_events(source, "rpc.demo.updated".to_owned());
        let first = filtered.next().await.expect("matching frame").expect("ok");
        assert_eq!(first.type_, "rpc.demo.updated");
        assert!(filtered.next().await.is_none(), "non-matching frames are skipped");
    }

    /// Minimal scripted SSE server (content-type text/event-stream,
    /// JSON-in-string double-decoded payloads).
    async fn serve_sse(body: &'static str) -> (Client, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let task = tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut request = vec![0u8; 8192];
            let _ = socket.read(&mut request).await;
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\ncontent-length: {}\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
        });
        (Client::new(&format!("http://{addr}")).expect("client"), task)
    }

    fn sse_payload(type_: &str) -> String {
        // /api/event frames are JSON-in-string (V2EventEncoded).
        let inner = serde_json::to_string(&json!({"type": type_, "data": {"n": 1}})).unwrap();
        let quoted = serde_json::to_string(&inner).unwrap();
        format!("data: {quoted}\n\n")
    }

    /// `on()` over the real /api/event seam: matching events reach the
    /// handler; dropping the guard unsubscribes.
    #[tokio::test]
    async fn on_forwards_matching_events_until_unsubscribed() {
        let body = format!(
            "{}{}{}{}",
            sse_payload("server.connected"),
            sse_payload("rpc.demo.updated"),
            sse_payload("session.updated"),
            sse_payload("rpc.demo.updated"),
        );
        let (client, server) = serve_sse(Box::leak(body.into_boxed_str())).await;
        let rpc = demo_definition().client(client);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let guard = rpc
            .on("updated", move |frame| {
                let tx = tx.clone();
                async move {
                    let _ = tx.send(frame.type_);
                }
            })
            .expect("subscribe");
        let mut seen = Vec::new();
        while seen.len() < 2 {
            seen.push(rx.recv().await.unwrap_or_else(|| panic!("got {:?} events", seen.len())));
        }
        assert_eq!(seen, ["rpc.demo.updated", "rpc.demo.updated"]);
        guard.unsubscribe();
        server.abort();
    }

    /// G-376①: two `on()` subscriptions join the client-wide shared
    /// `/api/event` multiplexer — one SSE connection total (the ref's
    /// `makeRpc` receives the client's single `SharedEvents` instance).
    #[tokio::test]
    async fn on_subscriptions_share_one_sse_connection() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let connections = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&connections);
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else { return };
                counter.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(async move {
                    use tokio::io::AsyncWriteExt;
                    let body = format!(
                        "{}{}",
                        sse_payload("server.connected"),
                        sse_payload("rpc.demo.updated")
                    );
                    let response =
                        format!("HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\r\n{body}");
                    let _ = socket.write_all(response.as_bytes()).await;
                    // Hold the connection open so both subscribers join it.
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                });
            }
        });
        let client = Client::new(&format!("http://{addr}")).expect("client");
        let rpc = demo_definition().client(client.clone());

        let (tx1, mut rx1) = tokio::sync::mpsc::unbounded_channel();
        let (tx2, mut rx2) = tokio::sync::mpsc::unbounded_channel();
        let first = rpc
            .on("updated", move |frame| {
                let tx = tx1.clone();
                async move {
                    let _ = tx.send(frame.type_);
                }
            })
            .expect("first subscribe");
        let second = rpc
            .on("updated", move |frame| {
                let tx = tx2.clone();
                async move {
                    let _ = tx.send(frame.type_);
                }
            })
            .expect("second subscribe");

        for received in [&mut rx1, &mut rx2] {
            let event = received.recv().await.unwrap_or_else(|| panic!("handler never fired"));
            assert_eq!(event, "rpc.demo.updated");
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert_eq!(
            connections.load(Ordering::SeqCst),
            1,
            "both on() subscriptions must share one SSE connection"
        );
        first.unsubscribe();
        second.unsubscribe();
        task.abort();
    }

    #[tokio::test]
    async fn typed_roundtrip_decodes_the_output_core() {
        let (client, server) = serve_once(200, r#"{"output":{"value":7}}"#).await;
        let output: EchoOutput = client
            .rpc()
            .call_into(
                "rpcID",
                "echo",
                &crate::types::RpcCallQuery { location: None },
                &json!({"a": 1}),
            )
            .await
            .expect("typed call");
        assert_eq!(output.value, 7);
        server.abort();
    }

    /// RE-06 loud failures: missing `output`, shape mismatch, and
    /// unknown fields (closed output schema) all error with the method
    /// named — never a silent `None`.
    #[tokio::test]
    async fn wire_drift_errors_loudly() {
        let (client, server) = serve_once(200, r#"{"output":null}"#).await;
        let error = client
            .rpc()
            .call_into::<serde_json::Value, EchoOutput>(
                "rpcID",
                "echo",
                &crate::types::RpcCallQuery { location: None },
                &json!({}),
            )
            .await
            .expect_err("null output must error");
        assert!(error.to_string().contains("echo"), "{error}");
        server.abort();

        let (client, server) = serve_once(200, r#"{"output":{"other":true}}"#).await;
        let error = client
            .rpc()
            .call_into::<serde_json::Value, EchoOutput>(
                "rpcID",
                "echo",
                &crate::types::RpcCallQuery { location: None },
                &json!({}),
            )
            .await
            .expect_err("shape drift must error");
        assert!(error.to_string().contains("echo"), "{error}");
        server.abort();
    }
}
