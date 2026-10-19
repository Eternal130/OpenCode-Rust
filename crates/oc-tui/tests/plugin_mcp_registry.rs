//! PLUG-D33 — plugin-declared MCP servers reach the REAL runtime
//! registry the app serves/queries. The ref's plugin host mutates
//! the MCP service editor directly (`ctx.mcp.transform`,
//! packages/core/src/plugin/host.ts:374-403); in the client/server
//! split the TUI plugin host carries the snapshot's servers to the
//! server's registry over the wire — `PUT /api/experimental/mcp/
//! {server}` (`McpRegistry::add`) — and the refreshed `mcp.list`
//! rows (the sidebar's MCP section + `/mcps` dialog source) become
//! plugin-aware.
//!
//! Hermetic: a scripted 127.0.0.1:0 listener answers the registry
//! seam (recording every request); a real bun-backed host loads the
//! `mcpreg` fixture; no real network, no 49374.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use oc_tui::plugin_boot::{fetch_mcp_rows, sync_and_fetch};
use oc_tui::reactive::Runtime;
use oc_tui::store::Store;

fn sandbox() -> PathBuf {
    let dir = PathBuf::from(format!("/tmp/opencode/oc-tui-mcp-reg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("sandbox");
    std::fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../oc-plugin-host/tests/fixtures/mcpreg.ts"),
        dir.join("mcpreg.ts"),
    )
    .expect("fixture copy");
    dir
}

/// The scripted registry listener: records every request line
/// (method + path); `PUT /api/experimental/mcp/{name}` adds to the
/// registry set; `DELETE` removes; `GET /api/mcp` lists the set (the
/// row shape the client parses).
async fn spawn_registry(
) -> (std::net::SocketAddr, Arc<Mutex<Vec<String>>>, Arc<Mutex<BTreeSet<String>>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let registry: Arc<Mutex<BTreeSet<String>>> = Arc::new(Mutex::new(BTreeSet::new()));
    {
        let log = Arc::clone(&log);
        let registry = Arc::clone(&registry);
        // The listener runs until the process ends (test scope; the
        // connections close with the client).
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else { break };
                let log = Arc::clone(&log);
                let registry = Arc::clone(&registry);
                tokio::spawn(async move {
                    let mut buffer = vec![0u8; 16384];
                    let mut read = 0usize;
                    // Read until the end of the HEAD (plus any body).
                    loop {
                        let got = match socket.read(&mut buffer[read..]).await {
                            Ok(0) | Err(_) => return,
                            Ok(got) => got,
                        };
                        read += got;
                        let text = String::from_utf8_lossy(&buffer[..read]);
                        if let Some(header_end) = text.find("\r\n\r\n") {
                            let length = text
                                .lines()
                                .find_map(|line| {
                                    let (name, value) = line.split_once(':')?;
                                    name.trim()
                                        .eq_ignore_ascii_case("content-length")
                                        .then(|| value.trim().parse::<usize>().ok())?
                                })
                                .unwrap_or(0);
                            if read >= header_end + 4 + length {
                                break;
                            }
                        }
                    }
                    let text = String::from_utf8_lossy(&buffer[..read]).to_string();
                    let method = text.split_whitespace().next().unwrap_or_default().to_owned();
                    let path = text
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or_default()
                        .split('?')
                        .next()
                        .unwrap_or_default()
                        .to_owned();
                    log.lock().expect("log").push(format!("{method} {path}"));
                    let response = if method == "PUT" && path.starts_with("/api/experimental/mcp/")
                    {
                        let name = path.trim_start_matches("/api/experimental/mcp/").to_owned();
                        registry.lock().expect("registry").insert(name);
                        "HTTP/1.1 204 No Content\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                            .to_owned()
                    } else if method == "DELETE" && path.starts_with("/api/experimental/mcp/") {
                        let name = path.trim_start_matches("/api/experimental/mcp/").to_owned();
                        registry.lock().expect("registry").remove(&name);
                        "HTTP/1.1 204 No Content\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                            .to_owned()
                    } else if method == "GET" && path == "/api/mcp" {
                        let rows: Vec<String> = registry
                            .lock()
                            .expect("registry")
                            .iter()
                            .map(|name| {
                                format!(r#"{{"name":"{name}","status":{{"status":"pending"}}}}"#)
                            })
                            .collect();
                        // The list wrapper's wire shape
                        // (`json_location_data`: `{location, data}`).
                        let body = format!(
                            "{{\"location\":{{\"directory\":\"/tmp/opencode\"}},\"data\":[{}]}}",
                            rows.join(",")
                        );
                        format!(
                            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                            body.len()
                        )
                    } else {
                        "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                            .to_owned()
                    };
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
    }
    (addr, log, registry)
}

/// The registry integration seam: the fixture's `ctx.mcp.transform`
/// server lands in the real registry over the wire (PUT), and the
/// refreshed `mcp.list` rows carry it into the store — the sidebar's
/// MCP section + `/mcps` dialog source see the plugin server.
#[tokio::test]
async fn plugin_mcp_registered() {
    let (addr, log, _registry) = spawn_registry().await;

    // Boot the real host with the mcpreg fixture.
    let sandbox = sandbox();
    let config = oc_plugin_host::PluginHostConfig {
        env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string())],
        ..oc_plugin_host::PluginHostConfig::default()
    };
    let host = Arc::new(
        oc_plugin_host::PluginHost::detect()
            .start_detached_with(config, &[sandbox.join("mcpreg.ts")]),
    );
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(20)), "host ready: {:?}", host.start_error());

    // The plugin's MCP materialized into the registration snapshot
    // (the guest's whole-table editor seam).
    let snapshot = host.registrations();
    let mcp = snapshot
        .mcps
        .iter()
        .find(|mcp| mcp.name == "plugindemo")
        .expect("the ctx.mcp.transform server landed in registrations.mcps");
    assert_eq!(mcp.plugin_id, "mcpreg");

    // The sync (the production seam `start_mcp_sync` drives): the
    // config reaches the REAL registry over the wire.
    let client = oc_client::Client::new(&format!("http://{addr}")).expect("client");
    let servers: std::collections::BTreeMap<String, serde_json::Value> =
        snapshot.mcps.iter().map(|mcp| (mcp.name.clone(), mcp.config.clone())).collect();
    let (rows, pushed) = sync_and_fetch(&client, servers, BTreeSet::new()).await;
    assert_eq!(pushed, ["plugindemo".to_owned()].into_iter().collect::<BTreeSet<_>>());

    let requests = log.lock().expect("log").clone();
    assert!(
        requests.iter().any(|request| request == "PUT /api/experimental/mcp/plugindemo"),
        "the plugin server reached the registry: {requests:?}"
    );
    assert!(
        requests.iter().any(|request| request == "GET /api/mcp"),
        "the refresh re-listed the registry: {requests:?}"
    );

    // The refreshed rows (server truth) make the plugin server
    // TUI-visible: the store's MCP table feeds the sidebar section
    // and the `/mcps` dialog.
    let row = rows.iter().find(|row| row.name == "plugindemo").unwrap_or_else(|| {
        panic!("the refreshed row; requests so far: {requests:?}; rows: {rows:?}")
    });
    assert_eq!(row.status, "pending");
    let store = Store::new(Runtime::new());
    store.set_mcp_servers(rows);
    let servers = store.mcp_servers();
    assert!(
        servers.iter().any(|server| server.name == "plugindemo"),
        "the sidebar's MCP table carries the plugin server: {servers:?}"
    );

    // The removal seam: an empty snapshot table (a hot reload dropped
    // the server) DELETEs it from the registry.
    let (rows, pushed) = sync_and_fetch(&client, std::collections::BTreeMap::new(), pushed).await;
    assert!(pushed.is_empty(), "the pushed set cleared: {pushed:?}");
    assert!(
        !rows.iter().any(|row| row.name == "plugindemo"),
        "the re-listed registry no longer carries the plugin server"
    );
    let requests = log.lock().expect("log").clone();
    assert!(
        requests.iter().any(|request| request == "DELETE /api/experimental/mcp/plugindemo"),
        "the removal reached the registry: {requests:?}"
    );

    // The bare refresh (the boot-fetch chain's mapping) parses the
    // same wire shape.
    let rows = fetch_mcp_rows(&client).await.expect("fetch");
    assert!(rows.is_empty(), "the registry is empty after the removal: {rows:?}");

    host.shutdown();
}

/// The failure path: an invalid `ServerConfig` declines THIS server
/// (the ref's `Schema.decodeUnknownSync(ServerConfig)` — host.ts:
/// 387-397) without crashing the pass or blocking healthy neighbors.
#[tokio::test]
async fn invalid_plugin_mcp_config_declines_contained() {
    let (addr, log, _registry) = spawn_registry().await;
    let client = oc_client::Client::new(&format!("http://{addr}")).expect("client");
    let mut servers = std::collections::BTreeMap::new();
    servers.insert("broken".to_owned(), serde_json::json!({ "type": "telepathy", "vibes": true }));
    servers
        .insert("healthy".to_owned(), serde_json::json!({ "type": "local", "command": ["cat"] }));
    let (rows, pushed) = sync_and_fetch(&client, servers, BTreeSet::new()).await;
    assert_eq!(pushed, ["healthy".to_owned()].into_iter().collect::<BTreeSet<_>>());
    let requests = log.lock().expect("log").clone();
    assert!(
        !requests.iter().any(|request| request.contains("broken")),
        "the invalid config never reaches the wire: {requests:?}"
    );
    assert!(rows.iter().any(|row| row.name == "healthy"), "the neighbor pushed: {rows:?}");
}
