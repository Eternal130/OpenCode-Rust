//! R-04 app-archive serving end-to-end: a fixture archive (built in-test
//! with the same brotli q6 the CI packager uses — no network, no
//! committed binary fixture) is installed into a real app shell and
//! driven over raw HTTP. Pins the exact header parity the direct-insert
//! tests in `app_shell.rs` already pin, plus the archive-specific paths
//! (lazy decompression, provenance banner).

mod support;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use oc_core::engine::{Engine, EngineOptions};
use oc_server::app::AppShell;
use oc_server::app_archive::{AppArchive, Source};
use oc_server::assets::csp_for_html;
use oc_server::lifecycle::{serve, ServeHandle, ServeOptions};
use oc_server::state::{AppState, AuthConfig};
use oc_util::determinism::{SystemClock, UuidV4IdGen};

use support::request;

const REF: &str = "e7a34f09bfd9134dfade5a8ddb843f7030bc9a69";

const INDEX_HTML: &[u8] = br#"<html><head>
<script id="oc-theme-preload-script">
  (() => { document.documentElement.dataset.theme = "dark" })()
</script>
<script src="/_assets/app.js"></script>
</head><body>shell</body></html>"#;

const APP_JS: &[u8] = b"console.log(1)";
const SW_JS: &[u8] = b"self.addEventListener('install', () => {});";

/// brotli q6 — the exact parameters `tools/web-assets/package.ts` uses
/// in CI, so the fixture bytes are production-shaped.
fn brotli6(plain: &[u8]) -> Vec<u8> {
    let params = brotli::enc::BrotliEncoderParams {
        quality: 6,
        ..brotli::enc::BrotliEncoderParams::default()
    };
    let mut reader = std::io::Cursor::new(plain.to_vec());
    let mut packed = Vec::new();
    brotli::enc::BrotliCompress(&mut reader, &mut packed, &params).expect("compress");
    packed
}

fn fixture_archive() -> AppArchive {
    let files = [("index.html", INDEX_HTML), ("_assets/app.js", APP_JS), ("sw.js", SW_JS)];
    let mut encoded = serde_json::Map::new();
    for (path, plain) in files {
        encoded.insert(
            path.to_owned(),
            serde_json::Value::String(
                base64::engine::general_purpose::STANDARD.encode(brotli6(plain)),
            ),
        );
    }
    let doc = serde_json::json!({
        "format": "opencode-rust-app-archive/1",
        "provenance": { "ref_commit": REF, "built_at": "2026-10-11T00:00:00Z", "app_commit": REF },
        "files": encoded,
    });
    AppArchive::from_bytes(doc.to_string().as_bytes(), Source::Embedded).expect("fixture archive")
}

struct Server {
    state: Arc<AppState>,
    handle: ServeHandle,
    addr: SocketAddr,
}

impl Server {
    /// Boots with the fixture archive installed (no direct entries —
    /// pure archive serving, like a release binary).
    async fn boot(archive: AppArchive) -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let engine =
            Engine::boot(EngineOptions::new(tmp.path().to_owned(), tmp.path().to_owned())).await;
        let mut state = AppState::new(
            engine,
            AuthConfig { password: Some("secret".to_owned()) },
            "test-version".to_owned(),
            Arc::new(UuidV4IdGen),
            Arc::new(SystemClock),
        );
        state.assets.set_archive(archive);
        let state = Arc::new(state);
        let shell = AppShell::new(Arc::clone(&state));
        let router = shell.router();
        let handle = serve(
            &ServeOptions { port: Some(0), ..ServeOptions::default() },
            Arc::clone(&state),
            router,
            None,
        )
        .await
        .expect("serve");
        std::mem::forget(tmp);
        let addr = handle.addr;
        Self { state, handle, addr }
    }

    async fn get(&self, target: &str) -> support::RawResponse {
        request(self.addr, "GET", target, &[], &[]).await.expect("request")
    }

    async fn shutdown(self) {
        self.handle.begin_shutdown();
        tokio::time::timeout(Duration::from_secs(10), self.handle.shutdown_and_join())
            .await
            .expect("server drains")
            .expect("serve ok");
    }
}

#[tokio::test]
async fn app_archive_serves_index_with_exact_header_parity() {
    let server = Server::boot(fixture_archive()).await;

    // Provenance banner: installed archive reports itself (the boot path
    // prints it to stderr; the store exposes the same line).
    assert_eq!(
        server.state.assets.provenance_line().as_deref(),
        Some("opencode-rust: web ui: 3 files (ref e7a34f09bfd9134dfade5a8ddb843f7030bc9a69)")
    );

    // `/` (unauthenticated — static serving precedes auth) serves the
    // DECOMPRESSED index.html bytes with the exact HTML header set:
    // no-cache revalidation, nosniff, and the CSP carrying the SHA-256
    // of the inline preload script.
    let response = server.get("/").await;
    assert_eq!(response.status, 200);
    assert_eq!(response.header("content-type"), Some("text/html"));
    assert_eq!(response.header("cache-control"), Some("no-cache"));
    assert_eq!(response.header("x-content-type-options"), Some("nosniff"));
    assert_eq!(response.body, INDEX_HTML);
    let expected_csp = csp_for_html(INDEX_HTML);
    assert_eq!(response.header("content-security-policy"), Some(expected_csp.as_str()));
    let csp = response.header("content-security-policy").expect("csp").to_owned();
    assert!(csp.contains("'sha256-"), "inline preload hash injected: {csp}");

    server.shutdown().await;
}

#[tokio::test]
async fn app_archive_asset_hits_immutable_and_misses_404_no_store() {
    let server = Server::boot(fixture_archive()).await;

    // Exact hit under _assets/: the immutable long-cache split.
    let response = server.get("/_assets/app.js").await;
    assert_eq!(response.status, 200);
    assert_eq!(response.header("content-type"), Some("text/javascript"));
    assert_eq!(response.header("cache-control"), Some("public, max-age=31536000, immutable"));
    assert_eq!(response.header("x-content-type-options"), Some("nosniff"));
    assert_eq!(response.body, APP_JS);

    // Misses under _assets/ and icons/: 404 no-store, never the SPA.
    for target in ["/_assets/missing.js", "/icons/icon.svg"] {
        let response = server.get(target).await;
        assert_eq!(response.status, 404, "{target}");
        assert_eq!(response.header("cache-control"), Some("no-store"), "{target}");
    }

    server.shutdown().await;
}

#[tokio::test]
async fn app_archive_spa_fallback_and_sw_revalidation() {
    let server = Server::boot(fixture_archive()).await;

    // Unknown non-asset path → the SPA shell (index.html bytes + HTML
    // headers), served before auth.
    let response = server.get("/foo/bar").await;
    assert_eq!(response.status, 200);
    assert_eq!(response.header("content-type"), Some("text/html"));
    assert_eq!(response.header("cache-control"), Some("no-cache"));
    assert_eq!(response.body, INDEX_HTML);

    // sw.js rides the revalidate (no-cache) branch like the index.
    let response = server.get("/sw.js").await;
    assert_eq!(response.status, 200);
    assert_eq!(response.header("content-type"), Some("text/javascript"));
    assert_eq!(response.header("cache-control"), Some("no-cache"));
    assert_eq!(response.body, SW_JS);

    server.shutdown().await;
}

#[tokio::test]
async fn app_archive_head_and_post_semantics() {
    let server = Server::boot(fixture_archive()).await;

    // HEAD: 200, the full header set, empty body, the DECOMPRESSED
    // length.
    let head = request(server.addr, "HEAD", "/", &[], &[]).await.expect("head");
    assert_eq!(head.status, 200);
    assert_eq!(head.body, [] as [u8; 0]);
    assert_eq!(
        head.header("content-length").map(str::parse::<usize>),
        Some(Ok(INDEX_HTML.len())),
        "content-length must be the decompressed size"
    );
    assert_eq!(head.header("cache-control"), Some("no-cache"));

    // POST: 405, empty body (GET/HEAD only).
    let post = request(server.addr, "POST", "/", &[], b"x").await.expect("post");
    assert_eq!(post.status, 405);
    assert_eq!(post.body, [] as [u8; 0]);

    server.shutdown().await;
}

#[tokio::test]
async fn app_archive_repeat_requests_stay_correct_through_the_cache() {
    let server = Server::boot(fixture_archive()).await;
    // Several rounds through the bounded decompression cache — hits,
    // misses and evictions must all serve identical bytes.
    for _ in 0..4 {
        for target in ["/", "/_assets/app.js", "/sw.js", "/", "/deep/route/x"] {
            let response = server.get(target).await;
            assert_eq!(response.status, 200, "{target}");
            let expected = if target == "/_assets/app.js" {
                APP_JS
            } else if target == "/sw.js" {
                SW_JS
            } else {
                INDEX_HTML
            };
            assert_eq!(response.body, expected, "{target}");
        }
    }
    server.shutdown().await;
}

/// The runtime override path accepts an unpacked dist directory
/// (`OPENCODE_APP_ASSETS`), not just a packaged archive — same serving
/// behavior, plain (undecorated) entries.
#[tokio::test]
async fn app_archive_dist_directory_override_serves_identically() {
    let root = tempfile::tempdir().expect("tempdir");
    std::fs::write(root.path().join("index.html"), INDEX_HTML).expect("write index");
    std::fs::create_dir(root.path().join("_assets")).expect("mkdir assets");
    std::fs::write(root.path().join("_assets").join("app.js"), APP_JS).expect("write js");

    let archive = AppArchive::load_directory(root.path(), Source::Override).expect("dist archive");
    let server = Server::boot(archive).await;

    let response = server.get("/").await;
    assert_eq!(response.status, 200);
    assert_eq!(response.body, INDEX_HTML);
    let expected_csp = csp_for_html(INDEX_HTML);
    assert_eq!(response.header("content-security-policy"), Some(expected_csp.as_str()));
    let response = server.get("/_assets/app.js").await;
    assert_eq!(response.status, 200);
    assert_eq!(response.body, APP_JS);

    server.shutdown().await;
}
