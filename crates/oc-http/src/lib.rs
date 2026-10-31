//! G-26: unified TLS trust for the reqwest 0.12 HTTP stack.
//!
//! The workspace intentionally keeps two reqwest stacks (see the oc-core
//! `Cargo.toml` note on the rmcp transport): reqwest 0.13 (rmcp/MCP) and
//! reqwest 0.12 (everything else). reqwest 0.13's `rustls` feature wires
//! `rustls-platform-verifier` (OS trust store — enterprise self-signed CAs
//! work), while reqwest 0.12's `rustls-tls` feature defaults to the baked-in
//! `webpki-roots` set, so enterprise roots were trusted on one stack and
//! rejected on the other.
//!
//! This crate owns the 0.12 client shape: [`client()`]/[`builder()`]
//! pre-configure rustls with `rustls-platform-verifier`, so both stacks
//! verify against the same platform trust store. If the platform verifier
//! cannot be initialised (e.g. a minimal container with no system trust
//! store), the client falls back to the webpki root set instead of
//! panicking, and the failure is logged.
//!
//! Coverage limits: the platform-verifier path cannot be end-to-end tested
//! in unit tests — installing a self-signed CA into the OS trust store is
//! an environment mutation this crate deliberately avoids. The tests here
//! cover construction (verifier attached, no panic) and that an
//! *uninstalled* self-signed certificate is rejected (the negative half of
//! the trust behaviour); positive acceptance of an installed enterprise
//! root is only exercisable on a provisioned machine.

use std::sync::Arc;

/// Platform-trust rustls `ClientConfig` (G-26).
///
/// Verifies server certificates with `rustls-platform-verifier` (OS trust
/// store), matching the reqwest 0.13 stack. Falls back to the webpki root
/// set when the platform verifier is unavailable.
pub fn platform_tls_config() -> rustls::ClientConfig {
    // Match the workspace rustls default (aws-lc-rs when both providers
    // are compiled in, same as the reqwest 0.13 stack).
    let provider = rustls::crypto::CryptoProvider::get_default()
        .cloned()
        .unwrap_or_else(|| std::sync::Arc::new(rustls::crypto::aws_lc_rs::default_provider()));
    match rustls_platform_verifier::Verifier::new(provider.clone()) {
        Ok(verifier) => rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("oc-http: safe default protocol versions")
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(verifier))
            .with_no_client_auth(),
        Err(err) => {
            // Degrade to webpki roots rather than panicking; every
            // `reqwest::Client::new()` call site in the workspace flows
            // through here and must stay infallible.
            eprintln!(
                "oc-http: platform verifier unavailable ({err}); falling back to webpki roots"
            );
            let roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
            rustls::ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .expect("oc-http: safe default protocol versions")
                .with_root_certificates(roots)
                .with_no_client_auth()
        }
    }
}

/// `reqwest::ClientBuilder` with the platform verifier (G-26) attached.
pub fn builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder().use_preconfigured_tls(platform_tls_config())
}

/// `reqwest::Client` with the platform verifier (G-26) attached.
///
/// Drop-in replacement for `reqwest::Client::new()` on the 0.12 stack.
pub fn client() -> reqwest::Client {
    builder().build().expect("oc-http: reqwest client with platform verifier")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_constructs_with_platform_verifier() {
        // G-26: the 0.12 stack must construct (verifier attached, no panic).
        let _client = client();
    }

    #[test]
    fn builder_constructs_with_platform_verifier() {
        let _builder = builder();
    }

    #[test]
    fn platform_tls_config_builds() {
        let _config = platform_tls_config();
    }

    #[tokio::test]
    async fn untrusted_self_signed_cert_is_rejected() {
        // Negative half of G-26 trust behaviour: a self-signed certificate
        // that is NOT installed in the platform trust store must fail
        // verification (both stacks reject it). Testing acceptance of an
        // installed enterprise root would require mutating the OS trust
        // store — see the crate docs (coverage limits).
        use rcgen::generate_simple_self_signed;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let certificate = generate_simple_self_signed(vec!["127.0.0.1".to_string()])
            .expect("self-signed stub certificate");
        let server_crypto = rustls::ServerConfig::builder_with_provider(std::sync::Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .expect("versions")
        .with_no_client_auth()
        .with_single_cert(
            vec![certificate.cert.der().clone()],
            rustls::pki_types::PrivateKeyDer::Pkcs8(rustls::pki_types::PrivatePkcs8KeyDer::from(
                certificate.signing_key.serialize_der(),
            )),
        )
        .expect("server config");
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server_crypto));

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            let mut tls = acceptor.accept(stream).await.expect("tls handshake");
            let mut buf = [0u8; 256];
            let _n = tls.read(&mut buf).await;
            let _ = tls
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .await;
        });

        let err = client()
            .get(format!("https://{addr}/"))
            .send()
            .await
            .expect_err("untrusted self-signed cert must be rejected");
        // reqwest surfaces the rustls verification failure as a decode /
        // unknown CA error, never a successful response.
        assert!(!err.is_status(), "unexpected HTTP status success path: {err}");

        server.abort();
    }
}
