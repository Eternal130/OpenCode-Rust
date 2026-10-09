//! Gate-6 F1 regression: `/api/fs/read` containment (v2 `FileSystem.resolve`
//! double fence). The traversal vectors that leaked before the fix:
//!
//! - URL-encoded `..%2Fsecret.txt` (one level up),
//! - nested `..%2F..%2Fsecret.txt` (two levels up),
//! - a symlink inside the workspace pointing outside,
//! - an absolute override (`%2Fetc%2Fhostname` replaces the base),
//!
//! all die 500 `UnknownError` like the reference; contained reads (including
//! a symlink that resolves back inside) keep working, and a contained
//! missing file stays 404 `FileNotFoundError`. Also pins the v2 wire shape
//! for `fs.list`: entries carry location-RELATIVE paths with a trailing `/`
//! on directories.

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
    /// The workspace (the fs location directory).
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

fn location_headers(workspace: &tempfile::TempDir) -> Vec<(&'static str, String)> {
    vec![("x-opencode-directory", workspace.path().display().to_string())]
}

#[tokio::test]
async fn fs_read_containment_and_list_shape() {
    let server = boot().await;
    let workspace = server.workspace.path().to_path_buf();

    // Fixtures: a contained file, secrets one and two levels UP the tree,
    // and an outside target for the symlink escape.
    std::fs::write(workspace.join("inside.txt"), "contained payload").expect("write");
    std::fs::create_dir_all(workspace.join("notes")).expect("notes dir");
    std::fs::write(workspace.join("notes").join("x.txt"), "nested payload").expect("write");
    let parent = workspace.parent().expect("parent").to_path_buf();
    let grandparent = parent.parent().expect("grandparent").to_path_buf();
    std::fs::write(parent.join("secret.txt"), "parent secret").expect("parent secret");
    std::fs::write(grandparent.join("secret2.txt"), "grandparent secret").expect("grandparent");
    let outside = parent.join("outside-target.md");
    std::fs::write(&outside, "outside secret").expect("outside target");
    std::os::unix::fs::symlink(&outside, workspace.join("escape.md")).expect("escape symlink");
    std::os::unix::fs::symlink("inside.txt", workspace.join("alias.txt")).expect("inside symlink");

    let header_strings = location_headers(&server.workspace);
    let headers: Vec<(&'static str, &str)> =
        header_strings.iter().map(|(name, value)| (*name, value.as_str())).collect();
    let get = |target: String| {
        let headers = headers.clone();
        async move { request(server.addr, "GET", &target, &headers, &[]).await.expect("request") }
    };

    // Contained reads keep working (plain + via an in-workspace symlink).
    let response = get("/api/fs/read/inside.txt".to_owned()).await;
    assert_eq!(response.status, 200);
    assert_eq!(response.text(), "contained payload");
    let response = get("/api/fs/read/alias.txt".to_owned()).await;
    assert_eq!(response.status, 200, "in-workspace symlink resolves inside");
    assert_eq!(response.text(), "contained payload");

    // URL-encoded single traversal: `..%2Fsecret.txt` dies 500.
    let response = get("/api/fs/read/..%2Fsecret.txt".to_owned()).await;
    assert_eq!(response.status, 500, "one-level traversal must die");
    assert_eq!(response.json()["_tag"], json!("UnknownError"));

    // Nested traversal: `..%2F..%2Fsecret2.txt` dies 500.
    let response = get("/api/fs/read/..%2F..%2Fsecret2.txt".to_owned()).await;
    assert_eq!(response.status, 500, "two-level traversal must die");
    assert_eq!(response.json()["_tag"], json!("UnknownError"));

    // Symlink escape: the link lives inside, its target does not — the
    // canonicalize fence catches it (500, never the outside bytes).
    let response = get("/api/fs/read/escape.md".to_owned()).await;
    assert_eq!(response.status, 500, "symlink escape must die");
    assert_eq!(response.json()["_tag"], json!("UnknownError"));

    // Absolute override: an absolute input replaces the base entirely.
    let response = get("/api/fs/read/%2Fetc%2Fhostname".to_owned()).await;
    assert_eq!(response.status, 500, "absolute override must die");
    assert_eq!(response.json()["_tag"], json!("UnknownError"));

    // A contained missing file stays a 404 FileNotFoundError (v2 maps the
    // realPath NotFound through NotFoundError → FileNotFoundError).
    let response = get("/api/fs/read/missing.txt".to_owned()).await;
    assert_eq!(response.status, 404);
    assert_eq!(response.json()["_tag"], json!("FileNotFoundError"));

    // `..` alone resolves to the filesystem root — outside the location.
    let response = get("/api/fs/read/..".to_owned()).await;
    assert_eq!(response.status, 500);

    // fs.list: v2 wire shape — entries RELATIVE to the location directory,
    // directories with a trailing separator.
    let response = get("/api/fs/list".to_owned()).await;
    assert_eq!(response.status, 200);
    let body = response.json();
    let entries: Vec<(String, String)> = body["data"]
        .as_array()
        .expect("entries")
        .iter()
        .map(|entry| {
            (
                entry["path"].as_str().expect("path").to_owned(),
                entry["type"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert!(entries.contains(&("alias.txt".to_owned(), "file".to_owned())), "{entries:?}");
    assert!(entries.contains(&("escape.md".to_owned(), "file".to_owned())), "{entries:?}");
    assert!(entries.contains(&("inside.txt".to_owned(), "file".to_owned())), "{entries:?}");
    assert!(entries.contains(&("notes/".to_owned(), "directory".to_owned())), "{entries:?}");
    // No absolute paths on the wire.
    assert!(
        entries.iter().all(|(path, _)| !path.starts_with('/')),
        "list paths must be relative, got {entries:?}"
    );

    // Listing a subdirectory keeps paths relative to the LOCATION root.
    let response = get("/api/fs/list?path=notes".to_owned()).await;
    assert_eq!(response.status, 200);
    let body = response.json();
    let paths: Vec<&str> = body["data"]
        .as_array()
        .expect("entries")
        .iter()
        .map(|e| e["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, vec!["notes/x.txt"]);
}
