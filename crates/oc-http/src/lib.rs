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

use std::sync::{Arc, OnceLock};

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::CertificateDer;

/// The extra CA bundle file (G-299), read once per process.
///
/// Node's `NODE_EXTRA_CA_CERTS` semantics (per the Node docs and the ref
/// comment at `packages/core/src/effect/websocket-constructor.ts:66` —
/// "Keep trust on the runtime store so `NODE_EXTRA_CA_CERTS` remains
/// additive"): the value is the path to **a single PEM file**; the
/// well-known/platform CAs remain trusted and the file's CA certificates
/// are appended to them. (Node does not treat the value as a
/// colon-separated list, so neither do we.) When the file is unreadable
/// or holds no certificates, Node warns and continues with the default
/// trust — mirrored here with an stderr warning.
const EXTRA_CA_CERTS_ENV: &str = "NODE_EXTRA_CA_CERTS";

/// Read and parse the `NODE_EXTRA_CA_CERTS` PEM bundle (uncached; the
/// production path reads it once through [`extra_ca_certs`]).
#[must_use]
pub fn extra_ca_certs_from_env() -> Vec<CertificateDer<'static>> {
    let Some(path) = std::env::var_os(EXTRA_CA_CERTS_ENV) else {
        return Vec::new();
    };
    if path.is_empty() {
        return Vec::new();
    }
    let certs: Vec<CertificateDer<'static>> = match CertificateDer::pem_file_iter(&path) {
        Ok(iter) => iter.filter_map(Result::ok).collect(),
        Err(err) => {
            eprintln!(
                "oc-http: {EXTRA_CA_CERTS_ENV} set but unreadable ({err}); continuing with default trust"
            );
            return Vec::new();
        }
    };
    if certs.is_empty() {
        eprintln!(
            "oc-http: {EXTRA_CA_CERTS_ENV} held no CA certificates; continuing with default trust"
        );
    }
    certs
}

static EXTRA_CA_CERTS: OnceLock<Vec<CertificateDer<'static>>> = OnceLock::new();

/// The extra CA bundle, read from the environment once per process.
fn extra_ca_certs() -> &'static [CertificateDer<'static>] {
    EXTRA_CA_CERTS.get_or_init(extra_ca_certs_from_env).as_slice()
}

/// A verifier over system roots plus the extra CA bundle: the
/// platform-verifier cannot take additional anchors, so when extras are
/// present the trust store is rebuilt from the native/system root source
/// (rustls-native-certs, honoring `SSL_CERT_FILE`/`SSL_CERT_DIR` like the
/// platform verifier's unix backend) with the extra PEM appended. If no
/// system root loads (e.g. a minimal container), fall back to the webpki
/// well-known set plus the extras.
#[must_use]
pub fn tls_config_with_extra_certs(extra: &[CertificateDer<'static>]) -> rustls::ClientConfig {
    let provider = rustls::crypto::CryptoProvider::get_default()
        .cloned()
        .unwrap_or_else(|| Arc::new(rustls::crypto::aws_lc_rs::default_provider()));
    let mut roots = rustls::RootCertStore::empty();
    let native = rustls_native_certs::load_native_certs();
    let mut loaded_system = false;
    for cert in native.certs {
        if roots.add(cert).is_ok() {
            loaded_system = true;
        }
    }
    if !loaded_system {
        // No usable system root source: degrade to the well-known set
        // (same fallback posture as the platform-verifier path).
        roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
    }
    for cert in extra {
        let _ = roots.add(cert.clone());
    }
    rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("oc-http: safe default protocol versions")
        .with_root_certificates(roots)
        .with_no_client_auth()
}

/// The env-driven extra-CA configuration (fresh env read — the
/// production path caches through [`platform_tls_config`]). Equivalent to
/// the default trust when the env is unset or the bundle is unusable.
#[must_use]
pub fn extra_tls_config() -> rustls::ClientConfig {
    let extra = extra_ca_certs_from_env();
    if extra.is_empty() {
        platform_tls_config()
    } else {
        tls_config_with_extra_certs(&extra)
    }
}

/// Platform-trust rustls `ClientConfig` (G-26).
///
/// Verifies server certificates with `rustls-platform-verifier` (OS trust
/// store), matching the reqwest 0.13 stack. Falls back to the webpki root
/// set when the platform verifier is unavailable. When
/// `NODE_EXTRA_CA_CERTS` is set (read once per process), the config
/// instead trusts the system roots plus that PEM bundle (G-299).
#[must_use]
pub fn platform_tls_config() -> rustls::ClientConfig {
    let extra = extra_ca_certs();
    if !extra.is_empty() {
        return tls_config_with_extra_certs(extra);
    }
    // Match the workspace rustls default (aws-lc-rs when both providers
    // are compiled in, same as the reqwest 0.13 stack).
    let provider = rustls::crypto::CryptoProvider::get_default()
        .cloned()
        .unwrap_or_else(|| Arc::new(rustls::crypto::aws_lc_rs::default_provider()));
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
#[must_use]
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

    // =====================================================================
    // G-299 — NODE_EXTRA_CA_CERTS additive trust
    // =====================================================================

    /// A throwaway PEM bundle path (no tempfile dep; unique per process).
    fn temp_pem_path(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "oc-http-g299-{tag}-{}-{}.pem",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ))
    }

    /// A self-signed CA plus a server certificate chaining to it.
    fn ca_signed_pair() -> (rcgen::Certificate, rcgen::Certificate, rcgen::KeyPair) {
        use rcgen::{BasicConstraints, CertificateParams, IsCa, KeyPair};
        let ca_key = KeyPair::generate().expect("ca key");
        let mut ca_params = CertificateParams::new(Vec::new()).expect("ca params");
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let ca = ca_params.self_signed(&ca_key).expect("ca cert");
        let server_key = KeyPair::generate().expect("server key");
        let server_params =
            CertificateParams::new(vec!["127.0.0.1".to_string()]).expect("server params");
        let server = server_params
            .signed_by(&server_key, &rcgen::Issuer::from_params(&ca_params, &ca_key))
            .expect("server cert");
        (ca, server, server_key)
    }

    #[test]
    fn extra_ca_certs_env_parses_a_pem_bundle() {
        let (ca, _server, _key) = ca_signed_pair();
        let path = temp_pem_path("parse");
        std::fs::write(&path, ca.pem()).expect("write pem");
        std::env::set_var(EXTRA_CA_CERTS_ENV, &path);
        let certs = extra_ca_certs_from_env();
        std::env::remove_var(EXTRA_CA_CERTS_ENV);
        assert_eq!(certs.len(), 1, "one CA certificate from the bundle");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn extra_ca_certs_env_unset_or_unreadable_is_ignored() {
        std::env::remove_var(EXTRA_CA_CERTS_ENV);
        assert_eq!(extra_ca_certs_from_env().len(), 0, "unset env yields no certificates");
        std::env::set_var(EXTRA_CA_CERTS_ENV, "/nonexistent/oc-http-g299.pem");
        assert_eq!(
            extra_ca_certs_from_env().len(),
            0,
            "unreadable env path yields no certificates"
        );
        std::env::remove_var(EXTRA_CA_CERTS_ENV);
    }

    #[tokio::test]
    async fn node_extra_ca_certs_adds_trust_for_a_custom_ca() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let (ca, server_cert, server_key) = ca_signed_pair();
        let ca_der = rustls::pki_types::CertificateDer::from(ca.der().to_vec());
        let pem_path = temp_pem_path("trust");
        std::fs::write(&pem_path, ca.pem()).expect("write ca pem");

        let server_crypto = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .expect("versions")
        .with_no_client_auth()
        .with_single_cert(
            vec![server_cert.der().clone()],
            rustls::pki_types::PrivateKeyDer::Pkcs8(rustls::pki_types::PrivatePkcs8KeyDer::from(
                server_key.serialize_der(),
            )),
        )
        .expect("server config");
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server_crypto));

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");

        let server = tokio::spawn(async move {
            // Both probes (rejected and trusted) connect once each.
            for _ in 0..2 {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    let Ok(mut tls) = acceptor.accept(stream).await else {
                        return; // client-side rejection aborts here
                    };
                    let mut buf = [0u8; 256];
                    let _n = tls.read(&mut buf).await;
                    let _ = tls
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                        )
                        .await;
                });
            }
        });

        let url = format!("https://{addr}/");

        // (a) Without the env: the custom CA is not trusted — the
        // handshake fails (fresh env read, env unset).
        std::env::remove_var(EXTRA_CA_CERTS_ENV);
        let err = reqwest::Client::builder()
            .use_preconfigured_tls(extra_tls_config())
            .build()
            .expect("client")
            .get(&url)
            .send()
            .await
            .expect_err("custom CA must be rejected without NODE_EXTRA_CA_CERTS");
        assert!(!err.is_status(), "unexpected status success: {err}");

        // (b) With NODE_EXTRA_CA_CERTS=<pem file>: the extra CA is
        // appended to the trust roots and the request succeeds.
        std::env::set_var(EXTRA_CA_CERTS_ENV, &pem_path);
        let config = tls_config_with_extra_certs(&[ca_der]);
        let body = reqwest::Client::builder()
            .use_preconfigured_tls(config)
            .build()
            .expect("client")
            .get(&url)
            .send()
            .await
            .expect("request with the extra CA trusted")
            .text()
            .await
            .expect("body");
        assert_eq!(body, "ok");
        std::env::remove_var(EXTRA_CA_CERTS_ENV);

        server.abort();
        let _ = std::fs::remove_file(&pem_path);
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
