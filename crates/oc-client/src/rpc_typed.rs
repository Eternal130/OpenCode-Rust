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

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::ClientError;

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

#[cfg(test)]
mod tests {
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
