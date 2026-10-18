//! Phase-10 error-path goldens: hit representative 4xx routes on a booted
//! reference server AND a booted Rust server side-by-side, normalize both
//! bodies through the canonical normalizer, and compare byte-for-byte
//! (canonical serialization of the normalized bodies). The checked-in
//! golden (`goldens/p10/error-paths.json`) pins each side's normalized
//! shape **and** the divergence report — so a new divergence fails the
//! test, and a fixed divergence also fails (stale finding) until the
//! golden is refreshed.
//!
//! Route panel (v2.0.24 behavior verified against a live reference boot):
//!
//! | # | case | reference |
//! |---|------|-----------|
//! | 1 | `GET /api/session/{unknown}` | 404 `SessionNotFoundError` envelope |
//! | 2 | `GET /api/session?cursor=…` | 400 `InvalidCursorError` |
//! | 3 | `GET /api/info` (no auth) | 401 `UnauthorizedError` envelope |
//! | 4 | `GET /api/nonexistent-route` | 404 empty body |
//! | 5 | `POST /api/session` bad payload type | 400 `InvalidRequestError` kind=Payload |
//! | 6 | `GET /api/session?limit=…` bad query type | 400 `InvalidRequestError` kind=Query |
//! | 7 | `DELETE /api/info` (method mismatch) | **404 empty body** (v2's Effect router has no 405) |
//!
//! Cases 5-7 were the Phase-10 findings P10-E1/E2/E3 (schema-validation
//! gap, query-validation gap, 405-vs-404) — CLOSED by the final
//! remediation sweep: `handlers/schema_check.rs` ports the Effect
//! `TreeFormatter` message shapes for `session.create` payloads and
//! `session.list` queries, and the dispatch rewrites axum's 405 to the
//! empty 404. All seven routes are byte-equal now.
//!
//! Opt-in: `#[ignore]`d AND `OC_P10=1` at runtime (needs bun + reference):
//!
//! ```text
//! OC_P10=1 cargo nextest run --run-ignored only -E 'test(p10_error_paths)'
//! ```

mod p10_common;

use std::time::Duration;

use anyhow::Result;
use serde_json::{json, Value};

use p10_common::{enabled, golden_dir, skip};
use parity::scenario::ScenarioHost;

/// One probe: exact request shape + the expected golden finding (if the
/// two servers are known to diverge here).
struct Probe {
    name: &'static str,
    method: &'static str,
    path: &'static str,
    body: Option<Value>,
    auth: bool,
    /// `None` = servers must agree byte-for-byte after normalization.
    finding: Option<&'static str>,
}

fn probes() -> Vec<Probe> {
    vec![
        Probe {
            name: "unknown_session_404",
            method: "GET",
            path: "/api/session/ses_UNKNOWNSESSIONID0000",
            body: None,
            auth: true,
            finding: None,
        },
        Probe {
            name: "invalid_cursor_400",
            method: "GET",
            path: "/api/session?cursor=@@invalid-cursor@@",
            body: None,
            auth: true,
            finding: None,
        },
        Probe {
            name: "unauthorized_401",
            method: "GET",
            path: "/api/info",
            body: None,
            auth: false,
            finding: None,
        },
        Probe {
            name: "unknown_route_404",
            method: "GET",
            path: "/api/nonexistent-route",
            body: None,
            auth: true,
            finding: None,
        },
        Probe {
            name: "payload_schema_reject_400",
            method: "POST",
            path: "/api/session",
            body: Some(json!({"title": 12345})),
            auth: true,
            // P10-E1 — CLOSED: the port 400s byte-equal (schema_check).
            finding: None,
        },
        Probe {
            name: "query_schema_reject_400",
            method: "GET",
            path: "/api/session?limit=notanumber",
            body: None,
            auth: true,
            // P10-E2 — CLOSED: the port 400s byte-equal (schema_check).
            finding: None,
        },
        Probe {
            name: "method_mismatch_delete_info",
            method: "DELETE",
            path: "/api/info",
            body: None,
            auth: true,
            // P10-E3 — CLOSED: the dispatch answers 404 empty byte-equal.
            finding: None,
        },
    ]
}

/// One side's normalized response shape.
async fn probe_side(
    base: &str,
    password: &str,
    sandbox_root: &str,
    probe: &Probe,
) -> Result<Value> {
    let auth = probe.auth.then(|| parity::minih::basic_auth("opencode", password));
    let body = probe.body.as_ref().map(ToString::to_string);
    let response = parity::minih::request(
        base,
        auth.as_deref(),
        probe.method,
        probe.path,
        body.as_deref(),
        "application/json",
        Duration::from_secs(30),
    )
    .await?;
    let env = parity::normalize::NormEnv::new().with_root(sandbox_root);
    let text = String::from_utf8_lossy(&response.body).into_owned();
    // Error bodies are pinned normalized; success bodies are elided (they
    // carry run-random slugs/ids that normalization does not fully mask —
    // their equality is covered by the scenario goldens, not this panel).
    let body_json: Value = serde_json::from_str(&text).unwrap_or(Value::String(text.clone()));
    let is_error = response.status >= 400 || body_json.get("_tag").is_some();
    let body = if text.trim().is_empty() {
        json!("<empty>")
    } else if is_error {
        parity::normalize::normalize_value(&body_json, &env)
    } else {
        json!("<success-body-elided>")
    };
    let content_type = response
        .header("content-type")
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    Ok(json!({
        "status": response.status,
        "content_type": content_type,
        "body": body,
    }))
}

#[tokio::test]
#[ignore = "Phase-10 battery: opt in with OC_P10=1 (needs bun + reference)"]
async fn p10_error_path_goldens_side_by_side() -> Result<()> {
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

async fn run(
    reference: &parity::reference::ReferenceServer,
    port: &parity::rust_server::RustServer,
) -> Result<()> {
    let mut report = Vec::new();
    let probes = probes();
    for probe in &probes {
        let ref_side = probe_side(
            reference.url(),
            reference.password(),
            &reference.sandbox().to_string_lossy(),
            probe,
        )
        .await?;
        let port_side =
            probe_side(port.url(), port.password(), &port.sandbox().to_string_lossy(), probe)
                .await?;

        // Byte-compare the NORMALIZED bodies (canonical serialization).
        let ref_bytes = serde_json::to_string(&ref_side)?;
        let port_bytes = serde_json::to_string(&port_side)?;
        let divergence = if ref_bytes == port_bytes {
            Value::Null
        } else if ref_side["status"] != port_side["status"] {
            json!(format!("status: reference={} port={}", ref_side["status"], port_side["status"]))
        } else {
            let detail = parity::httpdiff::deep_diff(&ref_side, &port_side)
                .map_or_else(|| "body differs".to_string(), |d| d.to_string());
            json!(format!("body: {detail}"))
        };

        report.push(json!({
            "name": probe.name,
            "request": {
                "method": probe.method,
                "path": probe.path,
                "body": probe.body.clone().unwrap_or(Value::Null),
                "auth": probe.auth,
            },
            "reference": ref_side,
            "port": port_side,
            "divergence": divergence,
            "finding": probe.finding,
        }));
    }

    let value = Value::Array(report);
    std::fs::create_dir_all(p10_common::artifacts())?;
    std::fs::write(
        p10_common::artifacts().join("error-paths.json"),
        format!("{}\n", serde_json::to_string_pretty(&value)?),
    )?;
    parity::golden::assert_json_in(&golden_dir(), "error-paths.json", &value)?;

    // Cross-check: every case without a pinned finding must be divergence-free.
    for entry in value.as_array().expect("report array") {
        let has_finding = entry["finding"].is_string();
        let diverged = !entry["divergence"].is_null();
        anyhow::ensure!(
            has_finding == diverged,
            "case {}: divergence state ({diverged}) does not match the pinned finding state \
             ({has_finding}) — refresh goldens/p10/error-paths.json with PARITY_UPDATE=1 after \
             classifying the change",
            entry["name"]
        );
    }
    eprintln!(
        "[p10-errors] PASS — {} routes pinned ({} divergent findings)",
        probes.len(),
        value.as_array().expect("array").iter().filter(|e| !e["divergence"].is_null()).count()
    );
    Ok(())
}
