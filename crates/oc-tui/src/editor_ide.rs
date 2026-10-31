//! G-20(1) — the IDE/editor socket (context/editor.ts:107-376 +
//! editor.ts:44-85): when an IDE (VS Code/Jetbrains via the Claude
//! Code bridge, or an `OPENCODE_EDITOR_SSE_PORT` host) exposes a
//! WebSocket JSON-RPC endpoint, the TUI connects, `initialize`s and
//! tracks `selection_changed` notifications — the SAME editor-context
//! seam the Zed sqlite poll feeds (`store.apply_zed_selection`).
//!
//! Discovery (editor.ts:44-85): `~/.claude/ide/*.lock` — the FILE
//! NAME is the port, the JSON body carries `transport` ("ws" only),
//! `workspaceFolders` (scored against the cwd — the longest
//! containing path wins, ties by mtime) and an optional `authToken`.
//!
//! Env override (editor.ts:116-119): `CLAUDE_CODE_SSE_PORT` or
//! `OPENCODE_EDITOR_SSE_PORT` — a valid port bypasses discovery
//! entirely (`resolveEditorConnection`, editor.ts:344-357).
//!
//! Auth (editor.ts:394-401): a lock-file token rides the
//! `x-claude-code-ide-authorization` header.

use std::path::{Path, PathBuf};

use crate::editor_zed::{ZedRange, ZedSelection};

/// One resolved IDE endpoint (`EditorConnection`, editor.ts:100-104).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdeConnection {
    pub port: u16,
    pub auth_token: Option<String>,
    /// `env:<port>` / `lock:<port>` (editor.ts:61/69).
    pub source: String,
}

/// The env override (`CLAUDE_CODE_SSE_PORT || OPENCODE_EDITOR_SSE_PORT`,
/// editor.ts:116-119): the first value that parses as a valid port.
#[must_use]
pub fn env_editor_port(claude_port: Option<&str>, opencode_port: Option<&str>) -> Option<u16> {
    // G-47 — the ref's `||` is FALSY: an EMPTY-STRING claude value
    // falls through to the OPENCODE candidate (editor.ts:118's
    // `CLAUDE_CODE_SSE_PORT || OPENCODE_EDITOR_SSE_PORT`); a
    // NON-EMPTY value that fails to parse stays chosen (the ref's
    // parseInt NaN — no fallback).
    let value = claude_port.filter(|value| !value.is_empty()).or(opencode_port)?;
    let parsed = value.trim().parse::<u16>().ok()?;
    (parsed > 0).then_some(parsed)
}

/// `discoverEditorConnection` (editor.ts:44-85): scan
/// `~/.claude/ide/*.lock`, keep `transport: "ws"` entries whose
/// workspace folders contain `directory`, best score then newest
/// mtime. `None` when the directory is unreadable or nothing matches.
#[must_use]
pub fn discover_ide_connection(home: Option<&str>, directory: &Path) -> Option<IdeConnection> {
    let root = PathBuf::from(home?).join(".claude").join("ide");
    let entries = std::fs::read_dir(&root).ok()?;
    let directory = resolve_path_buf(directory);
    let mut best: Option<(usize, std::time::SystemTime, IdeConnection)> = None;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(stem) = name.strip_suffix(".lock") else { continue };
        // The file name IS the port (editor.ts:56-58).
        let Ok(port) = stem.parse::<u16>() else { continue };
        if port == 0 {
            continue;
        }
        let file = entry.path();
        let Ok(text) = std::fs::read_to_string(&file) else { continue };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
        // `transport !== undefined && transport !== "ws"` drops.
        if let Some(transport) = value.get("transport") {
            if transport.as_str() != Some("ws") {
                continue;
            }
        }
        let folders = value
            .get("workspaceFolders")
            .and_then(|items| items.as_array())
            .map(|items| {
                items.iter().filter_map(|item| item.as_str().map(str::to_owned)).collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let score = folders
            .iter()
            .filter(|folder| contains(folder, &directory))
            .map(|folder| resolve_path(folder).len())
            .max()
            .unwrap_or(0);
        if score == 0 {
            continue;
        }
        let mtime = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        let candidate = (
            score,
            mtime,
            IdeConnection {
                port,
                auth_token: value
                    .get("authToken")
                    .and_then(|token| token.as_str())
                    .map(str::to_owned),
                source: format!("lock:{port}"),
            },
        );
        // `sort((l, r) => r.score - l.score || r.mtime - l.mtime)[0]`.
        if best.as_ref().is_none_or(|current| {
            candidate.0 > current.0 || (candidate.0 == current.0 && candidate.1 > current.1)
        }) {
            best = Some(candidate);
        }
    }
    best.map(|(_, _, connection)| connection)
}

/// `resolveEditorConnection` (editor.ts:344-357): the env port first,
/// else the lock discovery.
#[must_use]
pub fn resolve_ide_connection(home: Option<&str>, directory: &Path) -> Option<IdeConnection> {
    if let Some(port) = env_editor_port(
        std::env::var("CLAUDE_CODE_SSE_PORT").ok().as_deref(),
        std::env::var("OPENCODE_EDITOR_SSE_PORT").ok().as_deref(),
    ) {
        return Some(IdeConnection { port, auth_token: None, source: format!("env:{port}") });
    }
    discover_ide_connection(home, directory)
}

/// `decodeEditorSelection` (context/editor.ts:52-93): the ranges
/// form, or the single-selection form folded into one range.
#[must_use]
pub fn decode_selection(params: &serde_json::Value) -> Option<ZedSelection> {
    let object = params.as_object()?;
    let file_path = object.get("filePath")?.as_str()?.to_owned();
    let decode_position = |value: &serde_json::Value| -> Option<(u32, u32)> {
        let position = value.as_object()?;
        let coordinate = |key: &str| -> Option<u32> {
            let number = position.get(key)?.as_f64()?;
            (number >= 0.0 && number.fract() == 0.0).then(|| number as u32)
        };
        Some((coordinate("line")?, coordinate("character")?))
    };
    let decode_range = |range: &serde_json::Value| -> Option<ZedRange> {
        let range = range.as_object()?;
        let selection = range.get("selection")?.as_object()?;
        let (start_line, start_character) = decode_position(selection.get("start")?)?;
        let (end_line, end_character) = decode_position(selection.get("end")?)?;
        Some(ZedRange {
            text: range.get("text")?.as_str()?.to_owned(),
            start_line,
            start_character,
            end_line,
            end_character,
        })
    };
    if let Some(ranges) = object.get("ranges").and_then(|value| value.as_array()) {
        // `isMinLength(1)` — an empty array fails the decode.
        let decoded: Vec<ZedRange> = ranges.iter().filter_map(decode_range).collect();
        if decoded.is_empty() {
            return None;
        }
        return Some(ZedSelection { file_path, ranges: decoded });
    }
    // The single-selection form folds into one range
    // (EditorSelectionSchema's decodeTo transform).
    let text = object.get("text")?.as_str()?.to_owned();
    let (start_line, start_character) =
        object.get("selection").and_then(|sel| decode_position(sel.get("start")?))?;
    let (end_line, end_character) =
        object.get("selection").and_then(|sel| decode_position(sel.get("end")?))?;
    Some(ZedSelection {
        file_path,
        ranges: vec![ZedRange { text, start_line, start_character, end_line, end_character }],
    })
}

/// `pathContains`/`contains` (editor.ts:47-52): `directory` equals or
/// sits under `folder` (both resolved lexically).
fn contains(folder: &str, directory: &Path) -> bool {
    let folder = resolve_path_buf(Path::new(folder));
    let folder_parts: Vec<_> = folder.components().collect();
    let directory_parts: Vec<_> = directory.components().collect();
    folder_parts.len() <= directory_parts.len()
        && folder_parts.iter().zip(&directory_parts).all(|(left, right)| left == right)
}

/// Absolute, `.`/`..`-component-free path (path.resolve's lexical
/// equivalent — mirrors editor_zed's helper).
fn resolve_path_buf(value: &Path) -> PathBuf {
    let absolute = if value.is_absolute() {
        value.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")).join(value)
    };
    let mut cleaned = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                cleaned.pop();
            }
            other => cleaned.push(other),
        }
    }
    cleaned
}

fn resolve_path(value: &str) -> String {
    resolve_path_buf(Path::new(value)).display().to_string()
}

/// The editor socket task (`connect`, context/editor.ts:177-288 —
/// the WebSocket arm). Resolves the connection ONCE at boot (the env
/// override or the best lock file); `None` when neither exists (the
/// ref keeps polling for locks — the port re-resolves on reconnect
/// attempts within a run, not across). Emits every decoded
/// `selection_changed` selection; `None` = the socket closed
/// (the selection stays until a reconnect re-establishes it).
pub fn spawn_ide_socket() -> Option<tokio::sync::mpsc::Receiver<ZedSelection>> {
    let connection = resolve_ide_connection(
        std::env::var("HOME").ok().as_deref(),
        &std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    )?;
    let (tx, rx) = tokio::sync::mpsc::channel(8);
    tokio::spawn(async move {
        let mut attempt: u32 = 0;
        loop {
            let outcome = connect_once(&connection, &tx).await;
            if outcome.stopped {
                // The channel closed — the run loop ended.
                return;
            }
            // G-47 — the backoff RESETS on a successful connection
            // (`current.addEventListener("open", ... attempt = 0)`,
            // context/editor.ts:223): without it every reconnect
            // after the first jittered failure waits the CAPPED 10 s
            // forever. A session that OPENED and later closed starts
            // the ladder over at attempt 1 (1 s); only consecutive
            // failures keep climbing.
            if outcome.opened {
                attempt = 0;
            }
            // `scheduleReconnect` (context/editor.ts:278-282):
            // `min(1000 * 2^(attempt-1), 10_000)`.
            attempt += 1;
            let delay = std::time::Duration::from_millis(
                (1000u64.saturating_mul(1 << (attempt - 1).min(4))).min(10_000),
            );
            tokio::time::sleep(delay).await;
        }
    });
    Some(rx)
}

/// One connection attempt's outcome: `stopped` = the RECEIVER dropped
/// (the task ends); `opened` = the socket CONNECTED at least once
/// (G-47's backoff-reset signal).
struct ConnectOutcome {
    stopped: bool,
    opened: bool,
}

/// One connection attempt (`connect`'s socket arm).
async fn connect_once(
    connection: &IdeConnection,
    tx: &tokio::sync::mpsc::Sender<ZedSelection>,
) -> ConnectOutcome {
    use futures::{SinkExt as _, StreamExt as _};
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let url = format!("ws://127.0.0.1:{}", connection.port);
    let outcome = |stopped: bool, opened: bool| ConnectOutcome { stopped, opened };
    let Ok(mut request) = url.as_str().into_client_request() else {
        return outcome(tx.is_closed(), false);
    };
    // The auth header (editor.ts:394-401).
    if let Some(token) = &connection.auth_token {
        request
            .headers_mut()
            .insert("x-claude-code-ide-authorization", token.parse().expect("header value"));
    }
    let Ok((mut socket, _response)) = tokio_tungstenite::connect_async(request).await else {
        // The dial itself failed — the connection never OPENED (the
        // backoff keeps climbing).
        return outcome(tx.is_closed(), false);
    };
    // `initialize` (context/editor.ts:208-214) +
    // `notifications/initialized` on the reply (:271-275).
    let initialize = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "opencode", "version": "0.0.0" }
        }
    });
    if socket
        .send(tokio_tungstenite::tungstenite::Message::Text(initialize.to_string()))
        .await
        .is_err()
    {
        // The handshake write failed on an OPEN socket — still an
        // opened session for the reset rule.
        return outcome(tx.is_closed(), true);
    }
    while let Some(message) = socket.next().await {
        let text = match message {
            Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => text,
            Ok(tokio_tungstenite::tungstenite::Message::Close(_)) | Err(_) => break,
            Ok(_) => continue,
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
        let Some(body) = value.as_object() else { continue };
        match body.get("method").and_then(|method| method.as_str()) {
            // `selection_changed` (context/editor.ts:221-226).
            Some("selection_changed") => {
                if let Some(selection) = decode_selection(body.get("params").unwrap_or(&value)) {
                    if tx.send(selection).await.is_err() {
                        return outcome(true, true);
                    }
                }
            }
            _ => {
                // The initialize reply (id 1) completes the handshake
                // (:259-275): `notifications/initialized`.
                if body.get("id").is_some_and(|id| *id == serde_json::json!(1)) {
                    let _ = socket
                        .send(tokio_tungstenite::tungstenite::Message::Text(
                            serde_json::json!({
                                "jsonrpc": "2.0",
                                "method": "notifications/initialized"
                            })
                            .to_string(),
                        ))
                        .await;
                }
            }
        }
    }
    // The socket CLOSED after an open session (the reader loop broke
    // on Close/EOF) — reconnect with the ladder RESET.
    outcome(tx.is_closed(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_port_first_valid_wins() {
        // The ref parses the FIRST present value ONCE (editor.ts:
        // 116-119) — an invalid CLAUDE_CODE_SSE_PORT never falls
        // through to OPENCODE_EDITOR_SSE_PORT.
        assert_eq!(env_editor_port(Some("not-a-port"), Some("8081")), None);
        assert_eq!(env_editor_port(Some("9000"), Some("8081")), Some(9000));
        assert_eq!(env_editor_port(Some("0"), Some("0")), None);
        assert_eq!(env_editor_port(None, None), None);
        assert_eq!(env_editor_port(Some(""), Some("x")), None);
        // G-47 — the ref's `||` is falsy: an EMPTY-STRING claude
        // value falls through to the OPENCODE candidate.
        assert_eq!(env_editor_port(Some(""), Some("8081")), Some(8081));
        assert_eq!(env_editor_port(Some(""), Some("")), None);
        // Whitespace-only is TRUTHY in the ref (only "" falses) and
        // parseInt("  ") is NaN — no fallback.
        assert_eq!(env_editor_port(Some("  "), Some("8081")), None);
        // parseInt trims leading whitespace (Number.parseInt).
        assert_eq!(env_editor_port(Some(" 81"), None), Some(81));
    }

    /// G-47 — the backoff ladder RESETS after a successful connection
    /// (`open` → `attempt = 0`, context/editor.ts:223): a session
    /// that opened and closed reconnects at attempt 1 (1 s), while
    /// consecutive dial failures keep climbing to the 10 s cap.
    #[test]
    fn reconnect_backoff_resets_after_an_opened_session() {
        let delay = |opened: bool, attempt: u32| {
            // The loop's post-outcome math, mirrored.
            let attempt = if opened { 0 } else { attempt };
            let attempt = attempt + 1;
            (1000u64.saturating_mul(1 << (attempt - 1).min(4))).min(10_000)
        };
        // Consecutive failures climb: 1s, 2s, 4s, 8s, 10s, 10s.
        let ladder = (0..6).map(|attempt| delay(false, attempt)).collect::<Vec<_>>();
        assert_eq!(ladder, [1000, 2000, 4000, 8000, 10_000, 10_000]);
        // An OPENED session resets: the next reconnect waits 1 s —
        // even after the ladder had already capped at 10 s.
        assert_eq!(delay(true, 5), 1000);
    }

    #[test]
    fn selection_decodes_both_shapes() {
        let ranges = decode_selection(&serde_json::json!({
            "filePath": "/a/b.rs",
            "source": "websocket",
            "ranges": [
                { "text": "fn main", "selection": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 0, "character": 7 }
                }}
            ]
        }))
        .expect("ranges form decodes");
        assert_eq!(ranges.file_path, "/a/b.rs");
        assert_eq!(ranges.ranges.len(), 1);
        assert_eq!(ranges.ranges[0].text, "fn main");
        assert_eq!(ranges.ranges[0].end_character, 7);
        // The single-selection form folds into one range
        // (EditorSelectionSchema's decodeTo).
        let single = decode_selection(&serde_json::json!({
            "filePath": "/a/b.rs",
            "text": "x",
            "selection": {
                "start": { "line": 2, "character": 1 },
                "end": { "line": 3, "character": 4 }
            }
        }))
        .expect("single form decodes");
        assert_eq!(
            single.ranges,
            vec![ZedRange {
                text: "x".to_owned(),
                start_line: 2,
                start_character: 1,
                end_line: 3,
                end_character: 4,
            }]
        );
        // isMinLength(1): an empty ranges array fails.
        assert!(decode_selection(&serde_json::json!({
            "filePath": "/a", "ranges": []
        }))
        .is_none());
        assert!(decode_selection(&serde_json::json!({"filePath": "/a"})).is_none());
    }

    /// `discoverEditorConnection` against a real temp `~/.claude/ide`:
    /// the transport gate, the workspace scoring and the mtime
    /// tie-break (editor.ts:44-85).
    #[test]
    fn discovery_scores_workspaces_and_gates_transport() {
        let root = std::env::temp_dir().join(format!("oc-ide-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let ide = root.join(".claude").join("ide");
        std::fs::create_dir_all(&ide).expect("ide dir");
        let write_lock = |name: &str, body: &str| {
            std::fs::write(ide.join(name), body).expect("lock");
        };
        // Two matching locks: the LONGER containing workspace wins.
        let projects = root.join("projects").display().to_string();
        let project = root.join("projects/a").display().to_string();
        let src = root.join("projects/a/src").display().to_string();
        write_lock("40000.lock", &format!(r#"{{"workspaceFolders": ["{projects}"]}}"#));
        std::fs::create_dir_all(&src).expect("dir");
        write_lock("40001.lock", &format!(r#"{{"workspaceFolders": ["{project}"]}}"#));
        // A non-ws transport and a non-containing folder never score.
        write_lock(
            "40002.lock",
            &format!(r#"{{"transport": "sse", "workspaceFolders": ["{project}"]}}"#),
        );
        write_lock("40003.lock", r#"{"workspaceFolders": ["/elsewhere"]}"#);
        let found = discover_ide_connection(Some(root.to_str().expect("utf8")), Path::new(&src))
            .expect("a lock matches");
        assert_eq!(found.port, 40001);
        assert_eq!(found.source, "lock:40001");
        assert_eq!(found.auth_token, None);
        // The token round-trips when the lock carries one.
        write_lock(
            "40004.lock",
            &format!(r#"{{"workspaceFolders": ["{src}"], "authToken": "tok"}}"#),
        );
        let tokened = discover_ide_connection(Some(root.to_str().expect("utf8")), Path::new(&src))
            .expect("a lock matches");
        assert_eq!(tokened.port, 40004);
        assert_eq!(tokened.auth_token.as_deref(), Some("tok"));
        // A directory nothing contains finds nothing.
        assert_eq!(
            discover_ide_connection(Some(root.to_str().expect("utf8")), Path::new("/nowhere")),
            None
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
