//! RW-05: `/api/fs/find` semantics — protected-path exclusion (port of
//! core/filesystem/protected.ts) and the ranked `query` semantics the web
//! UI's fsFind relies on (substring completion first, fuzzy subsequence
//! fallback, default limit 50 like REF filesystem/search.ts).

mod support;

use std::sync::Arc;

use oc_core::engine::{Engine, EngineOptions};
use oc_server::handlers;
use oc_server::state::{AppState, AuthConfig};
use oc_util::determinism::{SystemClock, UuidV4IdGen};
use serde_json::json;

use support::request;

struct TestServer {
    addr: std::net::SocketAddr,
    workspace: tempfile::TempDir,
    _data: tempfile::TempDir,
}

async fn boot() -> TestServer {
    let workspace = tempfile::tempdir_in("/tmp/opencode").expect("workspace tempdir");
    let data = tempfile::tempdir_in("/tmp/opencode").expect("data tempdir");
    let engine =
        Engine::boot(EngineOptions::new(data.path().join("data"), data.path().join("home"))).await;
    let state = Arc::new(AppState::new(
        engine,
        AuthConfig { password: None },
        "test-version".to_owned(),
        Arc::new(UuidV4IdGen),
        Arc::new(SystemClock),
    ));
    let router = handlers::mount(Arc::clone(&state));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("ephemeral bind");
    let addr = listener.local_addr().expect("addr");
    let _ = state.local_addr.write().insert(addr);
    let _ = state.readiness.send(oc_server::state::Readiness::Ready);
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    TestServer { addr, workspace, _data: data }
}

async fn find_raw(server: &TestServer, query: &str) -> serde_json::Value {
    let workspace = server.workspace.path().display().to_string();
    let headers = [("x-opencode-directory", workspace.as_str())];
    let response =
        request(server.addr, "GET", &format!("/api/fs/find?query={query}"), &headers, &[])
            .await
            .expect("request");
    assert_eq!(response.status, 200, "{}", response.text());
    response.json()
}

/// The `data` array of the location envelope, as owned path strings.
fn paths(envelope: &serde_json::Value) -> Vec<String> {
    envelope["data"]
        .as_array()
        .expect("data array")
        .iter()
        .map(|entry| entry["path"].as_str().expect("path").to_owned())
        .collect()
}

#[tokio::test]
async fn find_substring_completion_ranks_first_and_fuzzy_falls_back() {
    let server = boot().await;
    let workspace = server.workspace.path().to_path_buf();
    std::fs::write(workspace.join("readme.md"), "x").expect("write");
    std::fs::create_dir_all(workspace.join("rust").join("examples").join("docs")).expect("dirs");
    std::fs::write(workspace.join("rust").join("examples").join("docs").join("todo.md"), "x")
        .expect("write");

    // Substring completion ("read" ⊂ "readme.md") must rank top even
    // though scattered subsequences also match (e.g. rust/.../docs).
    let envelope = find_raw(&server, "read").await;
    let found = paths(&envelope);
    assert!(!found.is_empty());
    assert!(found[0].ends_with("readme.md"), "substring completion first: {found:?}");

    // Fuzzy subsequence: "rdm" matches readme.md (tight: d-m adjacent)
    // and the scattered rust/.../todo.md path; the tighter match wins.
    let envelope = find_raw(&server, "rdm").await;
    let found = paths(&envelope);
    assert!(found.iter().any(|p| p.ends_with("readme.md")), "subsequence matches: {found:?}");
    assert!(found.iter().any(|p| p.ends_with("todo.md")), "subsequence matches: {found:?}");
    assert!(
        found.iter().position(|p| p.ends_with("readme.md"))
            < found.iter().position(|p| p.ends_with("todo.md")),
        "substring match outranks scattered subsequence: {found:?}"
    );

    // No match → empty data array.
    let envelope = find_raw(&server, "zzzznope").await;
    assert_eq!(paths(&envelope), Vec::<String>::new());
}

#[tokio::test]
async fn find_default_limit_50_and_explicit_limit() {
    let server = boot().await;
    let workspace = server.workspace.path().to_path_buf();
    for index in 0..60 {
        std::fs::write(workspace.join(format!("file{index:02}.txt")), "x").expect("write");
    }

    // REF search.ts: `input.limit ?? 50`.
    let envelope = find_raw(&server, "file").await;
    assert_eq!(paths(&envelope).len(), 50, "default limit is 50");

    let response = request(
        server.addr,
        "GET",
        "/api/fs/find?query=file&limit=3",
        &[("x-opencode-directory", workspace.display().to_string().as_str())],
        &[],
    )
    .await
    .expect("request");
    assert_eq!(response.status, 200);
    assert_eq!(paths(&response.json()).len(), 3, "explicit limit honored");
}

#[tokio::test]
async fn find_type_filter_and_entry_shape() {
    let server = boot().await;
    let workspace = server.workspace.path().to_path_buf();
    std::fs::create_dir_all(workspace.join("docs")).expect("docs dir");
    std::fs::write(workspace.join("docs.md"), "x").expect("write");

    for kind in ["file", "directory"] {
        let workspace_str = workspace.display().to_string();
        let response = request(
            server.addr,
            "GET",
            &format!("/api/fs/find?query=docs&type={kind}"),
            &[("x-opencode-directory", workspace_str.as_str())],
            &[],
        )
        .await
        .expect("request");
        assert_eq!(response.status, 200);
        let envelope = response.json();
        let entries = envelope["data"].as_array().expect("array");
        assert!(!entries.is_empty());
        for entry in entries {
            assert_eq!(entry["type"], json!(kind));
        }
    }
}

#[tokio::test]
async fn find_excludes_protected_and_heavy_trees() {
    let server = boot().await;
    let workspace = server.workspace.path().to_path_buf();
    // Heavy trees (pre-existing skip, ripgrep-default equivalents).
    for heavy in [".git", "node_modules", "target"] {
        std::fs::create_dir_all(workspace.join(heavy)).expect("heavy dir");
        std::fs::write(workspace.join(heavy).join("needle.txt"), "x").expect("write");
    }
    std::fs::write(workspace.join("needle.txt"), "x").expect("top-level hit");

    let envelope = find_raw(&server, "needle").await;
    let found = paths(&envelope);
    assert_eq!(found.len(), 1, "heavy trees excluded: {found:?}");
    assert!(found[0].ends_with("needle.txt"));

    // Home-rooted scans apply REF Protected.names(): on linux the table is
    // empty (mirroring REF exactly), so a home-rooted scan still finds
    // entries under e.g. ~/Desktop — the port keeps REF's platform gate
    // rather than inventing a linux table. End-to-end home scanning is not
    // fixture-safe; the table gating itself is unit-covered in
    // oc-core::filesystem::protected.
}
