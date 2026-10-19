//! Gate 1 — HTTP/SSE contract differential (PARITY-LOOP.md §8.3, B0-2
//! skeleton).
//!
//! Dual-end (reference v2.0.24 + Rust port) request/response compare of the
//! four §8.3 contract classes, honestly split by what can genuinely pass
//! today:
//!
//! - **(a) session/message pagination cursor** — live checks for the
//!   absent-vs-null envelope semantics (empty lists), default ordering
//!   (newest first), `limit` paging, cursor round-trips, and the
//!   `next`-cursor contract (present, string, direction `next`). The
//!   `previous`-cursor presence and opaque token payloads are **recorded
//!   divergences** (reference emits `previous` whenever a page is
//!   non-empty and packs different cursor fields; see `gap facts` in the
//!   artifact) — they are NOT asserted here because they are unregistered
//!   mismatches awaiting ledger triage (§8.5: registering them is not this
//!   lane's call).
//! - **(b) error envelope shape on a bad route** — live checks (404
//!   `SessionNotFoundError` envelope, 404 empty body on an unknown route).
//! - **(c) request-validation matrix** — table-driven over the pinned
//!   openapi document (`p10_openapi.rs` shows the pin): the HTTP-H0
//!   slice (11 probes over 10 routes — P10-E1/E2 pins + the C4b
//!   generated-table middleware seam, validate-first routes only) runs
//!   as real dual-end checks; every remaining operation is an explicit
//!   `pending` row in the matrix (structured gap, surfaced in the
//!   artifact — never a silent pass).
//! - **(d) public SSE event stream presence** — one subscription per side,
//!   first event must be `server.connected` with a normalized-equal
//!   payload.
//!
//! Opt-in: `#[ignore]`d AND `OC_P10=1` at runtime (needs bun + reference):
//!
//! ```text
//! OC_P10=1 cargo nextest run --run-ignored only -E 'test(p10_http_contract)'
//! ```
//!
//! Artifact: `/tmp/opencode/p10-artifacts/gate1-http-contract.json`.

mod p10_common;

use std::time::Duration;

use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};

use p10_common::{enabled, skip};
use parity::scenario::ScenarioHost;

const TIMEOUT: Duration = Duration::from_secs(30);

/// One side of the differential, preresolved.
struct Endpoints {
    base: String,
    password: String,
    label: &'static str,
}

impl std::fmt::Display for Endpoints {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label)
    }
}

async fn get(base: &str, password: &str, path: &str) -> Result<(u16, Value, String)> {
    let auth = parity::minih::basic_auth("opencode", password);
    let response =
        parity::minih::request(base, Some(&auth), "GET", path, None, "application/json", TIMEOUT)
            .await
            .with_context(|| format!("GET {path}"))?;
    let text = String::from_utf8_lossy(&response.body).into_owned();
    let value = serde_json::from_str::<Value>(&text).unwrap_or(Value::String(text.clone()));
    Ok((response.status, value, text))
}

async fn post_json(base: &str, password: &str, path: &str, body: &str) -> Result<(u16, Value)> {
    let auth = parity::minih::basic_auth("opencode", password);
    let response = parity::minih::request(
        base,
        Some(&auth),
        "POST",
        path,
        Some(body),
        "application/json",
        TIMEOUT,
    )
    .await
    .with_context(|| format!("POST {path}"))?;
    let text = String::from_utf8_lossy(&response.body).into_owned();
    let value = serde_json::from_str::<Value>(&text).unwrap_or(Value::String(text));
    Ok((response.status, value))
}

/// `Value::Null` PRESENT at `object[key]` (absent-vs-null is the contract).
fn null_present(object: &Value, key: &str) -> Result<()> {
    let raw = object.get(key);
    ensure!(
        raw == Some(&Value::Null),
        "expected {key:?} to be present-as-null (absent-vs-null contract), got {raw:?} in {object}"
    );
    Ok(())
}

fn titles(value: &Value) -> Vec<String> {
    value["data"]
        .as_array()
        .map(|rows| {
            rows.iter().map(|row| row["title"].as_str().unwrap_or_default().to_owned()).collect()
        })
        .unwrap_or_default()
}

/// Minimal base64url decode (no padding) — cursor tokens only; decode
/// failures are contract failures, not skips.
fn b64url_decode(token: &str) -> Result<Vec<u8>> {
    let mut bits = 0u32;
    let mut acc = 0u32;
    let mut out = Vec::new();
    for ch in token.bytes() {
        let sextet = match ch {
            b'A'..=b'Z' => u32::from(ch - b'A'),
            b'a'..=b'z' => u32::from(ch - b'a') + 26,
            b'0'..=b'9' => u32::from(ch - b'0') + 52,
            b'-' => 62,
            b'_' => 63,
            other => return Err(anyhow::anyhow!("non-base64url byte {other:#x} in cursor token")),
        };
        acc = (acc << 6) | sextet;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xff) as u8);
        }
    }
    Ok(out)
}

/// Decode one side's opaque list-cursor token into its JSON payload.
fn cursor_payload(token: &str) -> Result<Value> {
    let bytes = b64url_decode(token).context("decode list cursor token")?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    serde_json::from_str(&text).with_context(|| format!("cursor token payload {text:?}"))
}

/// The pinned openapi operation set (method, path), like `p10_openapi.rs`.
fn pinned_operations() -> Result<Vec<(String, String)>> {
    let pinned_path = p10_common::ref_source().join("packages/protocol/openapi.json");
    if !pinned_path.is_file()
        && parity::gate::skip_or_fail(&format!(
            "pinned openapi.json missing at {} (reference source checkout incomplete; \
             set PARITY_REF_SOURCE)",
            pinned_path.display()
        ))
    {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(&pinned_path)
        .with_context(|| format!("read {}", pinned_path.display()))?;
    let doc: Value = serde_json::from_str(&text).context("parse pinned openapi.json")?;
    let mut ops = Vec::new();
    for (path, methods) in doc["paths"].as_object().context("paths object")? {
        for (method, op) in methods.as_object().into_iter().flatten() {
            if matches!(
                method.as_str(),
                "get" | "put" | "post" | "delete" | "options" | "head" | "patch"
            ) && op.is_object()
            {
                ops.push((method.clone(), path.clone()));
            }
        }
    }
    ops.sort();
    Ok(ops)
}

/// The request-validation matrix rows for the implemented probes.
///
/// Rows 1–2 are the P10-E1/E2 battery pins (closed). Rows 3–10 are the
/// HTTP-H0 first slice (C4b spike): validate-first routes whose payload
/// 400s are enforced by the generated-table middleware
/// (`oc-server/src/request_validation.rs`), with messages byte-pinned
/// against a live v2.0.24 boot (evidence:
/// `/tmp/opencode/parity-evidence/HTTP-H0/ref-validation-messages*.json`).
const VALIDATED_ROUTES: &[(&str, &str, Option<&str>, &str)] = &[
    // (method, path-with-query, invalid body, note)
    ("POST", "/api/session", Some(r#"{"title": 12345}"#), "payload schema reject (P10-E1)"),
    ("GET", "/api/session?limit=notanumber", None, "query schema reject (P10-E2)"),
    (
        "POST",
        "/api/plugin/update",
        Some(r#"{"targets": "notanarray"}"#),
        "HTTP-H0 slice: required array type",
    ),
    (
        "POST",
        "/api/session/ses_P10H0/move",
        Some(r#"{"directory": 5}"#),
        "HTTP-H0 slice: required string",
    ),
    (
        "PATCH",
        "/api/credential/cred_P10H0",
        Some(r#"{"label": 5}"#),
        "HTTP-H0 slice: required string",
    ),
    ("POST", "/api/websearch", Some(r#"{"query": 5}"#), "HTTP-H0 slice: required string"),
    ("POST", "/api/shell", Some(r#"{"command": 5}"#), "HTTP-H0 slice: required string"),
    (
        "POST",
        "/api/shell",
        Some(r#"{"command": "ls", "timeout": -1}"#),
        "HTTP-H0 slice: integer minimum bound",
    ),
    ("POST", "/api/pty", Some(r#"{"command": 5}"#), "HTTP-H0 slice: required string"),
    ("PUT", "/api/pty/pty_P10H0", Some(r#"{"title": 5}"#), "HTTP-H0 slice: string type"),
    ("PATCH", "/api/project/prj_P10H0", Some(r#"{"name": 5}"#), "HTTP-H0 slice: string type"),
];

/// (method, template path) of every operation the probes above check —
/// concrete probe paths carry path-param VALUES while the pinned document
/// carries TEMPLATES; this list does the template-side bookkeeping.
const VALIDATED_TEMPLATE_OPS: &[(&str, &str)] = &[
    ("POST", "/api/session"),
    ("GET", "/api/session"),
    ("POST", "/api/plugin/update"),
    ("POST", "/api/session/{sessionID}/move"),
    ("PATCH", "/api/credential/{credentialID}"),
    ("POST", "/api/websearch"),
    ("POST", "/api/shell"),
    ("POST", "/api/pty"),
    ("PUT", "/api/pty/{ptyID}"),
    ("PATCH", "/api/project/{projectID}"),
];

/// Run one validation probe against one side, normalized like
/// `p10_error_paths.rs` (status + content-type + normalized error body).
async fn validation_probe_side(
    base: &str,
    password: &str,
    sandbox_root: &str,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> Result<Value> {
    let auth = parity::minih::basic_auth("opencode", password);
    let response =
        parity::minih::request(base, Some(&auth), method, path, body, "application/json", TIMEOUT)
            .await?;
    let env = parity::normalize::NormEnv::new().with_root(sandbox_root);
    let text = String::from_utf8_lossy(&response.body).into_owned();
    let body_json: Value = serde_json::from_str(&text).unwrap_or(Value::String(text.clone()));
    let normalized = if text.trim().is_empty() {
        json!("<empty>")
    } else {
        parity::normalize::normalize_value(&body_json, &env)
    };
    Ok(json!({
        "status": response.status,
        "content_type": response
            .header("content-type")
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim(),
        "body": normalized,
    }))
}

#[tokio::test]
#[ignore = "Phase-10 gate: opt in with OC_P10=1 (needs bun + reference)"]
async fn p10_http_contract_gate1() -> Result<()> {
    if !enabled() {
        skip("OC_P10 != 1");
        return Ok(());
    }
    if !p10_common::reference_runnable() {
        return Ok(());
    }
    if parity::rust_server::skip_if_no_cargo() {
        return Ok(());
    }

    let reference = parity::reference::spawn().await?;
    let port = parity::rust_server::spawn_rust(parity::rust_server::RustSpawn::default()).await?;
    let result = run(&reference, &port).await;
    let stop_ref = reference.stop();
    let stop_port = port.stop();
    result?;
    stop_ref.await?;
    stop_port.await?;
    Ok(())
}

#[allow(clippy::too_many_lines)]
async fn run(
    reference: &parity::reference::ReferenceServer,
    port: &parity::rust_server::RustServer,
) -> Result<()> {
    let sides = [
        Endpoints {
            base: reference.url().to_owned(),
            password: reference.password().to_owned(),
            label: "reference",
        },
        Endpoints {
            base: port.url().to_owned(),
            password: port.password().to_owned(),
            label: "port",
        },
    ];
    let mut report = json!({"gate": "gate1-http-contract"});
    let mut gap_facts: Vec<Value> = Vec::new();

    // =====================================================================
    // (a) pagination: absent-vs-null + order rules
    // =====================================================================
    let mut pagination = serde_json::Map::new();

    // Empty list: cursor keys PRESENT as null on both sides.
    for side in &sides {
        let (status, body, _) = get(&side.base, &side.password, "/api/session").await?;
        ensure!(status == 200, "[a] {side} list status {status}: {body}");
        ensure!(
            body["data"].as_array().is_some_and(Vec::is_empty),
            "[a] {side} fresh list not empty"
        );
        ensure!(body["cursor"].is_object(), "[a] {side} cursor envelope missing: {body}");
        null_present(&body["cursor"], "previous").context(format!("[a] {side} empty list"))?;
        null_present(&body["cursor"], "next").context(format!("[a] {side} empty list"))?;
    }
    pagination.insert("empty_list_cursor_null".to_owned(), json!("pass (both sides)"));

    // Create three sessions in a known order; default listing is newest
    // first on both sides, and no partial page => next cursor null.
    for name in ["gate1-a", "gate1-b", "gate1-c"] {
        for side in &sides {
            let (status, body) = post_json(
                &side.base,
                &side.password,
                "/api/session",
                &json!({ "title": name }).to_string(),
            )
            .await?;
            ensure!(status == 200, "[a] {side} create {name}: {status} {body}");
        }
    }
    for side in &sides {
        let (status, body, _) = get(&side.base, &side.password, "/api/session").await?;
        ensure!(status == 200, "[a] {side} list status {status}");
        let order = titles(&body);
        ensure!(
            order == ["gate1-c", "gate1-b", "gate1-a"],
            "[a] {side} default order is not newest-first: {order:?}"
        );
        // Gap fact (unregistered divergence, recorded not asserted): on a
        // NON-FULL page (3 rows < default limit) the reference still
        // encodes both cursors (it never checks has_more) while the port
        // nulls them — same class as the final-page fact below.
        gap_facts.push(json!({
            "section": "pagination",
            "fact": "non-full-page cursor presence",
            "side": side.label,
            "rows": order.len(),
            "cursor_next_is_string": body.pointer("/cursor/next").is_some_and(Value::is_string),
            "cursor_previous_is_string": body.pointer("/cursor/previous").is_some_and(Value::is_string),
        }));
    }
    pagination.insert("default_order_newest_first".to_owned(), json!("pass (both sides)"));

    // limit=2: both sides return exactly the newest two rows with a
    // string `next` cursor whose payload decodes to direction=next.
    let mut page1: Vec<(String, Value)> = Vec::new();
    for side in &sides {
        let (status, body, _) = get(&side.base, &side.password, "/api/session?limit=2").await?;
        ensure!(status == 200, "[a] {side} limit=2 status {status}");
        let order = titles(&body);
        ensure!(order == ["gate1-c", "gate1-b"], "[a] {side} limit=2 rows {order:?}");
        let next = body.pointer("/cursor/next").cloned().unwrap_or(Value::Null);
        ensure!(next.is_string(), "[a] {side} limit=2 next cursor not a string: {next:?}");
        let payload = cursor_payload(next.as_str().unwrap_or_default())?;
        ensure!(
            payload.pointer("/anchor/direction").and_then(Value::as_str) == Some("next"),
            "[a] {side} next cursor direction: {payload}"
        );
        page1.push((side.label.to_owned(), body.clone()));
        // Gap fact (unregistered divergence, recorded not asserted): the
        // reference also emits `previous` here (it encodes whenever a page
        // is non-empty) and the two cursor payloads carry different field
        // sets (the port adds explicit scope keys).
        let previous = body.pointer("/cursor/previous").cloned().unwrap_or(Value::Null);
        let keys: Vec<String> =
            payload.as_object().map(|object| object.keys().cloned().collect()).unwrap_or_default();
        gap_facts.push(json!({
            "section": "pagination",
            "fact": "limit-page cursor shape",
            "side": side.label,
            "cursor_previous_is_string": previous.is_string(),
            "cursor_payload_keys": keys,
        }));
    }
    pagination.insert("limit_page_rows_and_next_cursor".to_owned(), json!("pass (both sides)"));

    // Cursor round-trip: feed each side its OWN next cursor back; page two
    // holds the remaining row and never overlaps page one.
    for (side, entry) in sides.iter().zip(page1.iter()) {
        let body = &entry.1;
        let next = body.pointer("/cursor/next").and_then(Value::as_str).unwrap_or_default();
        let path = format!("/api/session?limit=2&cursor={next}");
        let (status, body2, _) = get(&side.base, &side.password, &path).await?;
        ensure!(status == 200, "[a] {side} cursor page-2 status {status}: {body2}");
        let order = titles(&body2);
        ensure!(order == ["gate1-a"], "[a] {side} cursor page-2 rows {order:?}");
        let p1: Vec<String> = titles(body);
        ensure!(
            order.iter().all(|title| !p1.contains(title)),
            "[a] {side} cursor pages overlap: {p1:?} vs {order:?}"
        );
        // Gap fact: on the final page the port nulls `next` while the
        // reference keeps encoding one (it never checks has_more here).
        let next2 = body2.pointer("/cursor/next").cloned().unwrap_or(Value::Null);
        gap_facts.push(json!({
            "section": "pagination",
            "fact": "final-page next cursor",
            "side": side.label,
            "cursor_next_is_string": next2.is_string(),
        }));
    }
    pagination.insert("cursor_roundtrip_no_overlap".to_owned(), json!("pass (both sides)"));

    // Empty-session message list: same null-vs-absent envelope on both.
    for side in &sides {
        let payload = json!({"title": "gate1-msg-empty"}).to_string();
        let (_, row) = post_json(&side.base, &side.password, "/api/session", &payload).await?;
        let id = row["data"]["id"].as_str().context("empty-msg session id")?.to_owned();
        let (status, body, _) =
            get(&side.base, &side.password, &format!("/api/session/{id}/message")).await?;
        ensure!(status == 200, "[a] {side} empty message list status {status}: {body}");
        ensure!(
            body["data"].as_array().is_some_and(Vec::is_empty),
            "[a] {side} messages not empty"
        );
        ensure!(body["cursor"].is_object(), "[a] {side} message cursor envelope missing: {body}");
        null_present(&body["cursor"], "previous").context(format!("[a] {side} empty messages"))?;
        null_present(&body["cursor"], "next").context(format!("[a] {side} empty messages"))?;
    }
    pagination.insert("empty_message_list_cursor_null".to_owned(), json!("pass (both sides)"));
    report["pagination"] = Value::Object(pagination);

    // =====================================================================
    // (b) error envelope shape on a bad route
    // =====================================================================
    let unknown = "ses_GATE1UNKNOWNID000000";
    let mut envelopes = Vec::new();
    for side in &sides {
        let (status, body, _) =
            get(&side.base, &side.password, &format!("/api/session/{unknown}")).await?;
        ensure!(status == 404, "[b] {side} unknown session status {status}: {body}");
        ensure!(body["_tag"] == json!("SessionNotFoundError"), "[b] {side} envelope tag: {body}");
        ensure!(body["sessionID"] == json!(unknown), "[b] {side} envelope sessionID: {body}");
        ensure!(
            body["message"] == json!(format!("Session not found: {unknown}")),
            "[b] {side} envelope message: {body}"
        );
        envelopes.push(body.clone());
    }
    ensure!(envelopes[0] == envelopes[1], "[b] error envelopes differ across sides: {envelopes:?}");
    for side in &sides {
        let (status, body, text) =
            get(&side.base, &side.password, "/api/gate1-no-such-route").await?;
        ensure!(status == 404, "[b] {side} unknown route status {status}: {text:?}");
        ensure!(text.trim().is_empty(), "[b] {side} unknown route body not empty: {text:?}");
        ensure!(body.is_string(), "[b] {side} unknown route body parsed unexpectedly: {body}");
    }
    report["error_envelope"] = json!({
        "unknown_session_404_envelope": "pass (both sides, byte-equal)",
        "unknown_route_404_empty": "pass (both sides)",
    });

    // =====================================================================
    // (c) request-validation matrix (table-driven over the pinned openapi)
    // =====================================================================
    let ops = pinned_operations().context("pinned operations")?;
    ensure!(!ops.is_empty(), "pinned openapi operation set is empty (probe skipped?)");
    let mut matrix: Vec<Value> = Vec::new();
    for (method, path, body, note) in VALIDATED_ROUTES {
        let reference_side = validation_probe_side(
            reference.url(),
            reference.password(),
            &reference.sandbox().to_string_lossy(),
            method,
            path,
            *body,
        )
        .await?;
        let port_side = validation_probe_side(
            port.url(),
            port.password(),
            &port.sandbox().to_string_lossy(),
            method,
            path,
            *body,
        )
        .await?;
        ensure!(
            reference_side == port_side,
            "[c] validation probe {method} {path} diverged:\nreference={reference_side}\nport={port_side}"
        );
        ensure!(
            reference_side["status"] == json!(400),
            "[c] validation probe {method} {path} status {} (want 400)",
            reference_side["status"]
        );
        matrix.push(json!({
            "method": method, "path": path, "state": "checked", "note": note,
            "normalized_response": reference_side,
        }));
    }
    let validated: Vec<(String, String)> = VALIDATED_TEMPLATE_OPS
        .iter()
        .map(|(method, path)| ((*method).to_owned(), (*path).to_owned()))
        .collect();
    let mut pending = 0;
    for (method, path) in &ops {
        if validated.iter().any(|(vm, vp)| vm.eq_ignore_ascii_case(method) && vp == path) {
            continue;
        }
        pending += 1;
        matrix.push(json!({
            "method": method, "path": path, "state": "pending",
            "reason": "request-validation class not yet triaged (B0-2 skeleton; wiring these \
                       probes needs per-route payload design + ledger coverage)",
        }));
    }
    ensure!(
        pending + VALIDATED_TEMPLATE_OPS.len() == ops.len(),
        "[c] matrix accounting: {pending} pending + {} checked != pinned operations {}",
        VALIDATED_TEMPLATE_OPS.len(),
        ops.len()
    );
    report["validation_matrix"] = json!({
        "pinned_operations": ops.len(),
        "checked": VALIDATED_ROUTES.len(),
        "checked_operations": VALIDATED_TEMPLATE_OPS.len(),
        "pending": pending,
        "rows": matrix,
    });
    eprintln!(
        "[gate1] validation matrix: {} probes covering {}/{} operations, {pending} pending \
              (structured gaps)",
        VALIDATED_ROUTES.len(),
        VALIDATED_TEMPLATE_OPS.len(),
        ops.len()
    );

    // =====================================================================
    // (d) public SSE event stream presence (one subscription per side)
    // =====================================================================
    let mut first: Vec<Value> = Vec::new();
    for (base, password, env) in [
        (reference.url(), reference.password(), reference.norm_env()),
        (port.url(), port.password(), port.norm_env()),
    ] {
        let mut stream =
            parity::sse::SseStream::subscribe(base, password, "/api/event", TIMEOUT).await?;
        let frame =
            stream.next_frame(Duration::from_secs(20)).await.context("[d] first SSE frame")?;
        ensure!(frame.is_data(), "[d] first frame is not a data frame: {frame:?}");
        let payload: Value = serde_json::from_str(frame.data.trim())
            .with_context(|| format!("[d] first frame payload not JSON: {:?}", frame.data))?;
        ensure!(
            payload["type"] == json!("server.connected"),
            "[d] first event type not server.connected: {payload}"
        );
        first.push(parity::normalize::normalize_value(&payload, &env));
    }
    ensure!(first[0] == first[1], "[d] normalized server.connected differs: {first:?}");
    report["sse_presence"] = json!({
        "first_event": "server.connected",
        "normalized_equal": true,
    });

    // =====================================================================
    // gaps + artifact
    // =====================================================================
    report["gap_facts"] = json!(gap_facts);
    report["gaps"] = json!({
        "pagination_previous_cursor_presence": "unregistered divergence (reference emits \
         `previous` on every non-empty page; port only after a cursor param) — recorded as \
         gap facts, awaiting ledger triage",
        "pagination_cursor_token_payload": "opaque cursor payloads carry different field sets \
         (port adds scope keys) — recorded as gap facts, awaiting ledger triage",
        "validation_matrix_pending": format!("{pending} operations structured as pending rows"),
    });
    std::fs::create_dir_all(p10_common::artifacts())?;
    std::fs::write(
        p10_common::artifacts().join("gate1-http-contract.json"),
        format!("{}\n", serde_json::to_string_pretty(&report)?),
    )?;
    eprintln!(
        "[gate1] PASS — pagination/envelope/matrix({checked}/{ops})/sse; {} gap facts recorded",
        gap_facts.len(),
        checked = VALIDATED_ROUTES.len(),
        ops = ops.len()
    );
    Ok(())
}
