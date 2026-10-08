//! Parity acceptance: projector fidelity against the pinned reference
//! (opencode v2.0.24, run with bun).
//!
//! (a) Boot the reference server (pattern: `database/tests/bun.rs` /
//!     `tools/parity`), drive the `session_lifecycle` scenario steps over
//!     the HTTP API in a sandbox project: create → rename → prompt (steer,
//!     no model ⇒ failed turn) → prompt (queue, stays pending) → create a
//!     second session → delete it.
//! (b) Open the reference-produced DB with Rust, read the durable event
//!     log through `Bus::log`, and REPLAY every durable event (global
//!     commit order) through the Rust session projector into a fresh
//!     Rust database.
//! (c) Assert the projected `session_v2` / `session_message` /
//!     `session_inbox` rows equal the reference DB's rows (semantic
//!     column compare; times and ids are deterministic from the replayed
//!     events).
//!
//! Skipped when bun or the reference checkout is absent.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss, // epoch-ms/rowcount casts in the comparator
    clippy::too_many_lines,
    clippy::items_after_statements,
    clippy::format_push_string,
    clippy::while_let_loop,
    clippy::uninlined_format_args,
)]

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use base64::Engine as _;
use futures::StreamExt;
use rusqlite::OptionalExtension;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};

use crate::bus::{Bus, BusOptions, LogInput, LogItem, ReplayOptions, SerializedEvent};
use crate::database::Database;
use crate::session::projector::install_projectors;

const REF_ROOT: &str = "/tmp/opencode/ref";
const SANDBOX_ROOT: &str = "/tmp/opencode/w4a-sandbox";
const FORBIDDEN_PORT: u16 = 49374;
const REF_PASSWORD: &str = "core-w4a-ref-password";

fn has_bun() -> bool {
    bun_path().is_some() && Path::new(REF_ROOT).is_dir()
}

fn bun_path() -> Option<PathBuf> {
    for candidate in ["/root/.bun/bin/bun", "/usr/local/bin/bun", "/usr/bin/bun"] {
        if Path::new(candidate).is_file() {
            return Some(PathBuf::from(candidate));
        }
    }
    std::env::var("PATH").ok().and_then(|paths| {
        paths.split(':').find_map(|dir| {
            let candidate = PathBuf::from(dir).join("bun");
            (candidate.is_file()).then_some(candidate)
        })
    })
}

fn free_port() -> Option<u16> {
    for _ in 0..16 {
        let listener = TcpListener::bind("127.0.0.1:0").ok()?;
        let port = listener.local_addr().ok()?.port();
        drop(listener);
        if port != FORBIDDEN_PORT {
            return Some(port);
        }
    }
    None
}

struct ReferenceServer {
    child: Child,
    home: PathBuf,
    port: u16,
}

impl ReferenceServer {
    async fn stop(mut self) {
        if let Some(pgid) = self.child.id() {
            let _ =
                std::process::Command::new("kill").args(["-TERM", &format!("-{pgid}")]).status();
        }
        if tokio::time::timeout(Duration::from_secs(5), self.child.wait())
            .await
            .is_ok_and(|status| status.is_ok())
        {
            return;
        }
        let _ = self.child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(5), self.child.wait()).await;
    }
}

fn prepare_sandbox(sandbox: &Path) -> std::io::Result<(PathBuf, PathBuf)> {
    let home = sandbox.join("home");
    let tmp = sandbox.join("tmp");
    for dir in [
        home.join(".local/share"),
        home.join(".local/state"),
        home.join(".config"),
        home.join(".cache"),
        tmp.clone(),
        sandbox.join("logs"),
        home.join("project"),
    ] {
        std::fs::create_dir_all(dir)?;
    }
    Ok((home, tmp))
}

/// The reference `serve` command runs the bus with `persist: false`
/// (memory-only events; only the workerd entry enables persistence). This
/// launcher starts the same server with `events: { persist: true }` so the
/// durable log lands in the database. Written into the reference workspace
/// (module resolution) and removed when the test ends.
const LAUNCHER: &str = r#"
import { Effect, Scope } from "effect"
import { ServerProcess } from "@opencode/server/process"

const port = Number(process.env.W4A_PORT ?? 46000)
const program = Effect.gen(function* () {
  const scope = yield* Scope.make()
  const server = yield* ServerProcess.start({
    app: { name: "cli", version: "2.0.24", channel: "stable" },
    hostname: "127.0.0.1",
    port,
    password: process.env.OPENCODE_PASSWORD,
    database: { path: process.env.OPENCODE_DB },
    events: { persist: true },
    models: { fetch: false },
    fs: { filewatcher: false },
  }).pipe(Effect.provideService(Scope.Scope, scope))
  const address = server.address
  console.log(`server listening on http://${address.address}:${address.port}`)
  yield* Effect.never
})

Effect.runPromise(program as Effect.Effect<never, unknown, never>).catch((error) => {
  console.error("w4a launcher failed", error)
  process.exit(1)
})
"#;

async fn spawn_reference() -> Result<(ReferenceServer, std::path::PathBuf), String> {
    let bun = bun_path().ok_or("bun not found")?;
    let root = PathBuf::from(REF_ROOT);
    let port = free_port().ok_or("no free port")?;
    let sandbox = PathBuf::from(SANDBOX_ROOT).join(format!("ref-{}-{port}", std::process::id()));
    let (home, tmp) = prepare_sandbox(&sandbox).map_err(|e| e.to_string())?;
    let db_path = home.join(".local/share/opencode").join("w4a-parity.db");
    let launcher = root.join("packages/cli").join("w4a-parity-serve.ts");
    std::fs::write(&launcher, LAUNCHER).map_err(|e| format!("write launcher: {e}"))?;

    let mut command = Command::new(bun);
    command
        .current_dir(&root)
        .arg("run")
        .arg("--cwd")
        .arg(root.join("packages/cli"))
        .arg("w4a-parity-serve.ts")
        .env_clear()
        .env("PATH", "/root/.bun/bin:/usr/local/bin:/usr/bin:/bin")
        .env("HOME", &home)
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("TMPDIR", &tmp)
        .env("OPENCODE_PASSWORD", REF_PASSWORD)
        .env("OPENCODE_VERSION", "2.0.24")
        .env("OPENCODE_DISABLE_MODELS_FETCH", "1")
        .env("W4A_PORT", port.to_string())
        .env("OPENCODE_DB", &db_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .process_group(0);
    let mut child = command.spawn().map_err(|e| format!("spawn: {e}"))?;
    let logs = sandbox.join("logs");
    tee_to_log(child.stderr.take().expect("stderr piped"), logs.join("stderr.log"));
    tee_to_log(child.stdout.take().expect("stdout piped"), logs.join("stdout.log"));

    let deadline = Duration::from_secs(90);
    let started = Instant::now();
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            let _ = std::fs::remove_file(&launcher);
            return Err(format!(
                "reference exited during boot ({status}); logs at {}",
                logs.display()
            ));
        }
        if started.elapsed() > deadline {
            let _ = std::fs::remove_file(&launcher);
            return Err(format!("reference not ready after {}s", deadline.as_secs()));
        }
        if info_status(port).await == Some(200) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Ok((ReferenceServer { child, home, port }, launcher))
}

fn tee_to_log(pipe: impl tokio::io::AsyncRead + Send + Unpin + 'static, path: PathBuf) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(pipe).lines();
        let mut sink = std::fs::File::create(&path).ok();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(sink) = sink.as_mut() {
                use std::io::Write;
                let _ = writeln!(sink, "{line}");
            }
        }
    });
}

use tokio::io::AsyncBufReadExt as _;

async fn info_status(port: u16) -> Option<u16> {
    let future = async {
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.ok()?;
        let auth =
            base64::engine::general_purpose::STANDARD.encode(format!("opencode:{REF_PASSWORD}"));
        let request = format!(
            "GET /api/info HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Basic {auth}\r\nAccept: */*\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(request.as_bytes()).await.ok()?;
        let mut head = [0u8; 128];
        let read = stream.read(&mut head).await.ok()?;
        let line = String::from_utf8_lossy(&head[..read]);
        line.split_whitespace().nth(1)?.parse::<u16>().ok()
    };
    tokio::time::timeout(Duration::from_secs(2), future).await.ok()?
}

/// Minimal JSON-body HTTP client returning (status, body). Reads exactly
/// Content-Length bytes (the server keeps connections alive).
async fn request(
    port: u16,
    method: &str,
    path: &str,
    body: Option<serde_json::Value>,
) -> Result<(u16, serde_json::Value), String> {
    let auth = base64::engine::general_purpose::STANDARD.encode(format!("opencode:{REF_PASSWORD}"));
    let payload = body.map(|value| value.to_string()).unwrap_or_default();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Basic {auth}\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
    tokio::time::timeout(Duration::from_secs(30), async {
        let mut stream =
            tokio::net::TcpStream::connect(("127.0.0.1", port)).await.map_err(|e| e.to_string())?;
        stream.write_all(request.as_bytes()).await.map_err(|e| e.to_string())?;
        let mut raw = Vec::new();
        let mut buffer = [0u8; 8192];
        loop {
            let read = stream.read(&mut buffer).await.map_err(|e| e.to_string())?;
            if read == 0 {
                break;
            }
            raw.extend_from_slice(&buffer[..read]);
            if let Some(response) = parse_response(&raw) {
                return Ok(response);
            }
        }
        // EOF-delimited (chunked) responses complete at connection close.
        parse_response_at_eof(&raw).ok_or_else(|| "incomplete response".to_string())
    })
    .await
    .map_err(|_| "request timed out".to_string())?
}

type Parsed = (u16, serde_json::Value);

/// A complete length-delimited response, when its body arrived.
fn parse_response(raw: &[u8]) -> Option<Parsed> {
    let head_end = raw.windows(4).position(|window| window == b"\r\n\r\n")?;
    let head = std::str::from_utf8(&raw[..head_end]).ok()?;
    let chunked = head.lines().any(|line| line.eq_ignore_ascii_case("transfer-encoding: chunked"));
    if chunked {
        return None;
    }
    let content_length = content_length(head)?;
    let body = &raw[head_end + 4..];
    if body.len() >= content_length {
        Some(response_value(head, &body[..content_length]))
    } else {
        None
    }
}

/// Accepts any head+body once the connection closed (chunked responses).
fn parse_response_at_eof(raw: &[u8]) -> Option<Parsed> {
    let head_end = raw.windows(4).position(|window| window == b"\r\n\r\n")?;
    let head = std::str::from_utf8(&raw[..head_end]).ok()?;
    let body = decode_chunked(&raw[head_end + 4..]);
    Some(response_value(head, &body))
}

fn content_length(head: &str) -> Option<usize> {
    head.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        (name.eq_ignore_ascii_case("content-length"))
            .then_some(value.trim().parse::<usize>().ok())?
    })
}

fn response_value(head: &str, body: &[u8]) -> Parsed {
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or_default();
    let value = if body.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(body).unwrap_or(serde_json::Value::Null)
    };
    (status, value)
}

/// Decodes a chunked body (RFC 7230 §4.1); non-chunked input passes through.
fn decode_chunked(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut cursor = raw;
    loop {
        let Some(line_end) = cursor.windows(2).position(|window| window == b"\r\n") else { break };
        let size = std::str::from_utf8(&cursor[..line_end]).ok().and_then(|text| {
            usize::from_str_radix(text.trim().split(';').next().unwrap_or(""), 16).ok()
        });
        let Some(size) = size else {
            return raw.to_vec();
        };
        cursor = &cursor[line_end + 2..];
        if size == 0 {
            break;
        }
        if cursor.len() < size {
            return raw.to_vec();
        }
        out.extend_from_slice(&cursor[..size]);
        cursor = &cursor[size..];
        if cursor.starts_with(b"\r\n") {
            cursor = &cursor[2..];
        }
    }
    out
}

/// Percent-encodes a path for `?location[directory]=…` queries.
fn percent_encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Semantic row comparison
// ---------------------------------------------------------------------------

/// Reads every row of `table` as ordered JSON objects keyed by column name,
/// with numbers normalized (int/float unify) and JSON-typed columns parsed.
fn semantic_rows(conn: &rusqlite::Connection, table: &str) -> Vec<serde_json::Value> {
    let mut statement =
        conn.prepare(&format!("SELECT * FROM {table} ORDER BY id")).expect("prepare");
    let columns: Vec<String> =
        statement.column_names().iter().map(|name| (*name).to_string()).collect();
    let rows = statement
        .query_map([], |row| {
            let mut object = serde_json::Map::new();
            for (index, column) in columns.iter().enumerate() {
                let value: rusqlite::types::Value = row.get(index)?;
                let value = match value {
                    rusqlite::types::Value::Null => serde_json::Value::Null,
                    rusqlite::types::Value::Integer(value) => serde_json::json!(value as f64),
                    rusqlite::types::Value::Real(value) => serde_json::json!(value),
                    rusqlite::types::Value::Text(value) => {
                        match serde_json::from_str::<serde_json::Value>(&value) {
                            // JSON columns compare parsed (key-order-insensitive
                            // through serde_json::Value equality).
                            Ok(parsed) => normalize(parsed),
                            Err(_) => serde_json::json!(value),
                        }
                    }
                    rusqlite::types::Value::Blob(value) => {
                        serde_json::json!(String::from_utf8_lossy(&value))
                    }
                };
                object.insert(column.clone(), value);
            }
            Ok(serde_json::Value::Object(object))
        })
        .expect("query")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("rows");
    rows
}

fn normalize(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Number(number) => number
            .as_f64()
            .map(|float| {
                if float.fract() == 0.0 && float.abs() < 1e15 {
                    serde_json::json!(float as i64)
                } else {
                    serde_json::json!(float)
                }
            })
            .unwrap_or(serde_json::Value::Number(number)),
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(normalize).collect())
        }
        serde_json::Value::Object(object) => serde_json::Value::Object(
            object.into_iter().map(|(key, value)| (key, normalize(value))).collect(),
        ),
        other => other,
    }
}

// ---------------------------------------------------------------------------
// The gate
// ---------------------------------------------------------------------------

#[tokio::test]
async fn session_projector_replay_parities_reference() {
    if !has_bun() {
        eprintln!("skipping: bun or reference checkout absent");
        return;
    }
    let (server, launcher) = spawn_reference().await.expect("spawn reference");
    struct LauncherGuard(PathBuf);
    impl Drop for LauncherGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let _launcher_guard = LauncherGuard(launcher);
    let port = server.port;

    // --- (a) drive the session_lifecycle scenario steps ---------------
    let project_dir = server.home.join("project").display().to_string();
    // Boot the project location first (the parity driver's warm-up GET);
    // session-scoped routes then resolve the session's location.
    let encoded = percent_encode(&project_dir);
    let mut warm: (u16, serde_json::Value) = (0, serde_json::Value::Null);
    for _ in 0..100 {
        warm = request(port, "GET", &format!("/api/location?location[directory]={encoded}"), None)
            .await
            .unwrap_or((0, serde_json::Value::Null));
        if warm.0 == 200 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert_eq!(warm.0, 200, "location warm-up: {}", warm.1);
    let (status, body) = request(
        port,
        "POST",
        "/api/session",
        Some(serde_json::json!({
            "title": "parity lifecycle",
            "location": { "directory": project_dir }
        })),
    )
    .await
    .expect("create session");
    assert_eq!(status, 200, "create: {body}");
    let session_id =
        body["data"]["id"].as_str().or(body["id"].as_str()).expect("session id").to_string();
    eprintln!("[w4a-parity] session {session_id}");

    let mut rename: (u16, serde_json::Value) = (0, serde_json::Value::Null);
    for _ in 0..150 {
        rename = request(
            port,
            "PATCH",
            &format!("/api/session/{session_id}"),
            Some(serde_json::json!({ "title": "parity renamed" })),
        )
        .await
        .unwrap_or((0, serde_json::Value::Null));
        if rename.0 == 200 {
            break;
        }
        // Location provisioning races the create; retry until warm.
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(rename.0 == 200 || rename.0 == 204, "rename: {}", rename.1);

    // A prompt with no model configured: admission succeeds, the turn
    // fails — the failed drain still exercises inbox delivery, the user
    // message projection, execution terminals and the idle marker.
    let _ = request(
        port,
        "POST",
        &format!("/api/session/{session_id}/prompt"),
        Some(serde_json::json!({ "text": "Say hello." })),
    )
    .await;

    // A queued prompt stays pending (the inbox row must survive).
    let _ = request(
        port,
        "POST",
        &format!("/api/session/{session_id}/prompt"),
        Some(serde_json::json!({ "text": "Next turn.", "delivery": "queue" })),
    )
    .await;

    // Quiescence: the failed turn's messages are readable.
    let mut settled = false;
    for _ in 0..100 {
        let (status, body) =
            request(port, "GET", &format!("/api/session/{session_id}/message"), None)
                .await
                .unwrap_or((0, serde_json::Value::Null));
        let messages = body["data"].as_array().or(body.as_array()).cloned().unwrap_or_default();
        if status == 200 && messages.iter().any(|message| message["type"] == "idle") {
            settled = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(settled, "failed turn reached the idle marker");

    // A queued prompt admitted WITHOUT waking execution stays pending — the
    // reference's inbox row must survive in ours too.
    let _ = request(
        port,
        "POST",
        &format!("/api/session/{session_id}/prompt"),
        Some(serde_json::json!({ "text": "Parked.", "delivery": "queue", "resume": false })),
    )
    .await;

    // The delete leg of the lifecycle on a second session.
    let (_, second) = request(
        port,
        "POST",
        "/api/session",
        Some(serde_json::json!({
            "title": "parity doomed",
            "location": { "directory": project_dir }
        })),
    )
    .await
    .expect("create second session");
    let second_id =
        second["data"]["id"].as_str().or(second["id"].as_str()).expect("second id").to_string();
    let (status, _) =
        request(port, "DELETE", &format!("/api/session/{second_id}"), None).await.expect("delete");
    assert!(status == 204 || status == 200, "delete: {status}");

    let db_file = server.home.join(".local/share/opencode").join("w4a-parity.db");
    server.stop().await;
    assert!(db_file.is_file(), "reference db at {}", db_file.display());

    // --- (b) read the reference log through Bus::log ------------------
    let reference = Database::open_file(&db_file).await.expect("open reference db");
    let reader =
        Bus::new(reference.clone(), BusOptions { persist: false, ..BusOptions::deterministic() });

    let aggregates: Vec<String> = reference
        .with_conn(|conn| {
            let mut statement =
                conn.prepare("SELECT DISTINCT aggregate_id FROM event ORDER BY aggregate_id")?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await
        .expect("list aggregates");
    assert!(
        aggregates.contains(&session_id),
        "the driven session must have durable events: {aggregates:?}"
    );

    let mut replay_queue: Vec<SerializedEvent> = Vec::new();
    for aggregate in &aggregates {
        let mut stream =
            reader.log(LogInput { aggregate_id: aggregate.clone(), after: None, follow: false });
        while let Some(item) = stream.next().await {
            match item.expect("log read") {
                LogItem::Event(event) => {
                    let durable = event.durable.expect("durable log event");
                    replay_queue.push(SerializedEvent {
                        id: event.id,
                        type_: format!("{}.{}", event.type_, durable.version),
                        created: Some(event.created),
                        seq: i64::try_from(durable.seq).unwrap_or(0),
                        aggregate_id: aggregate.clone(),
                        data: event.data,
                    });
                }
                LogItem::Synced(_) => break,
            }
        }
    }
    assert!(!replay_queue.is_empty(), "reference log has durable events");
    eprintln!(
        "[w4a-parity] {} durable events across {} aggregates",
        replay_queue.len(),
        aggregates.len()
    );

    // --- (c) replay through the Rust projector into a fresh DB --------
    let sandbox = PathBuf::from(SANDBOX_ROOT).join(format!("rust-{}", std::process::id()));
    std::fs::create_dir_all(&sandbox).expect("rust sandbox");
    let rust_db = Database::open_file(sandbox.join("opencode.db")).await.expect("fresh rust db");
    let bus =
        Bus::new(rust_db.clone(), BusOptions { persist: false, ..BusOptions::deterministic() });
    install_projectors(&bus);

    // The projection joins `project`; copy the reference's rows so foreign
    // keys hold during replay.
    let project_rows: Vec<Vec<Option<String>>> = reference
        .with_conn(|conn| {
            let mut statement = conn.prepare(
                "SELECT id, worktree, vcs, name, icon_url, icon_url_override, icon_color, \
                 CAST(time_initialized AS TEXT), CAST(time_active AS TEXT), sandboxes, commands \
                 FROM project",
            )?;
            let rows = statement
                .query_map([], |row| {
                    Ok((0..11)
                        .map(|index| row.get::<_, Option<String>>(index))
                        .collect::<rusqlite::Result<Vec<Option<String>>>>()
                        .unwrap())
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await
        .expect("copy project rows");
    rust_db
        .transaction(move |tx| {
            for row in &project_rows {
                tx.execute(
                    "INSERT OR IGNORE INTO project (id, worktree, vcs, name, icon_url, \
                     icon_url_override, icon_color, time_created, time_updated, time_initialized, \
                     time_active, sandboxes, commands) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, 0, ?8, ?9, ?10, ?11)",
                    rusqlite::params![
                        row[0], row[1], row[2], row[3], row[4], row[5], row[6], row[7], row[8],
                        row[9], row[10]
                    ],
                )?;
            }
            Ok(())
        })
        .await
        .expect("seed project rows");

    for event in replay_queue {
        bus.replay(event, ReplayOptions::default()).await.expect("replay event");
    }

    // --- compare projected rows semantically ---------------------------
    let reference_conn = rusqlite::Connection::open(&db_file).expect("reference conn");
    let rust_conn = rusqlite::Connection::open(sandbox.join("opencode.db")).expect("rust conn");

    for table in ["session_v2", "session_message", "session_inbox"] {
        let reference_rows = semantic_rows(&reference_conn, table);
        let rust_rows = semantic_rows(&rust_conn, table);
        assert_eq!(
            rust_rows.len(),
            reference_rows.len(),
            "{table} projection must parity the reference row count"
        );
        for (rust, reference) in rust_rows.iter().zip(&reference_rows) {
            assert_eq!(rust["id"], reference["id"], "{table} row ids must match");
            let mut rust_columns = rust.as_object().expect("row object").clone();
            let mut reference_columns = reference.as_object().expect("row object").clone();
            // `session_message.time_updated` is drizzle's wall-clock
            // `$onUpdate` stamp in the reference (not event-derived); the
            // replay pins it to the writing event's `created`. Compare it
            // with a small tolerance instead of exact equality.
            if table == "session_message" {
                let rust_stamp = rust_columns.remove("time_updated").unwrap_or_default();
                let reference_stamp = reference_columns.remove("time_updated").unwrap_or_default();
                let (rust_ms, reference_ms) = (
                    rust_stamp.as_f64().unwrap_or_default(),
                    reference_stamp.as_f64().unwrap_or_default(),
                );
                assert!(
                    (rust_ms - reference_ms).abs() <= 25.0,
                    "{table} {} time_updated drifted: {rust_ms} vs {reference_ms}",
                    rust["id"]
                );
            }
            assert_eq!(
                serde_json::Value::Object(rust_columns),
                serde_json::Value::Object(reference_columns),
                "{table} {} must parity the reference column-for-column",
                rust["id"]
            );
        }
        eprintln!("[w4a-parity] {table}: {} rows parities", reference_rows.len());
    }

    // The parked queued prompt left a pending row in both databases (the
    // inbox compare below asserts its full column parity).
    let parked: i64 = reference_conn
        .query_row("SELECT count(*) FROM session_inbox", [], |row| row.get(0))
        .unwrap();
    assert!(parked >= 1, "the parked prompt left a pending inbox row");

    // The deleted session is absent in both (already asserted by the table
    // compare, but make the leg explicit).
    let deleted: Option<String> = rust_conn
        .query_row("SELECT id FROM session_v2 WHERE id = ?1", rusqlite::params![second_id], |row| {
            row.get(0)
        })
        .optional()
        .unwrap();
    assert!(deleted.is_none(), "deleted session stays deleted after replay");
}
