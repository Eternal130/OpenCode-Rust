//! Phase-10 openapi equivalence gate: boot the Rust server, GET
//! `/openapi.json`, and semantic-diff it against the **pinned reference
//! document** (`/opt/opencode-ref/packages/protocol/openapi.json`).
//!
//! Semantic means (see `parity::openapi_diff`): path × method sets equal;
//! per-operation `operationId`, canonical parameter lists, `$ref` sets
//! under parameters / requestBody / each response status, and response
//! status sets equal; `components.schemas` name sets equal. Object key
//! order is not semantic. The difference list must be EMPTY minus the
//! checked-in whitelist (`goldens/p10/openapi-whitelist.json`), and the
//! golden summary (`goldens/p10/openapi-summary.json`) pins the operation /
//! schema counts and the byte-identity of the served document.
//!
//! Opt-in: `#[ignore]`d AND `OC_P10=1` at runtime (bun/reference optional —
//! this test needs the Rust server and the pinned file only):
//!
//! ```text
//! OC_P10=1 cargo nextest run --run-ignored only -E 'test(p10_openapi)'
//! ```

mod p10_common;

use anyhow::{Context, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};

use p10_common::{enabled, golden_dir, ref_source, skip};
use parity::scenario::ScenarioHost;

/// Run the gate. Returns the evidence summary on success.
#[tokio::test]
#[ignore = "Phase-10 battery: opt in with OC_P10=1"]
#[allow(clippy::too_many_lines)]
async fn p10_openapi_semantic_equivalence() -> Result<()> {
    if !enabled() {
        skip("OC_P10 != 1");
        return Ok(());
    }
    if parity::rust_server::skip_if_no_cargo() {
        return Ok(());
    }
    let pinned_path = ref_source().join("packages/protocol/openapi.json");
    if !pinned_path.is_file()
        && parity::gate::skip_or_fail(&format!(
            "pinned openapi.json missing at {} (reference source checkout incomplete; \
             set PARITY_REF_SOURCE)",
            pinned_path.display()
        ))
    {
        return Ok(());
    }

    // -- boot the Rust server and fetch the served document ---------------
    let server = parity::rust_server::spawn_rust(parity::rust_server::RustSpawn::default()).await?;
    let url = server.url().to_owned();
    let password = server.password().to_owned();
    let fetch = || async {
        let auth = parity::minih::basic_auth("opencode", &password);
        parity::minih::request(
            &url,
            Some(&auth),
            "GET",
            "/openapi.json",
            None,
            "application/json",
            std::time::Duration::from_secs(30),
        )
        .await
    };
    let served_response = fetch().await.context("GET /openapi.json from the Rust server")?;
    server.stop().await?;
    anyhow::ensure!(
        served_response.status == 200,
        "GET /openapi.json returned {}",
        served_response.status
    );

    let pinned_bytes =
        std::fs::read(&pinned_path).with_context(|| format!("read {}", pinned_path.display()))?;
    let served_text = String::from_utf8_lossy(&served_response.body).into_owned();
    let pinned_text = String::from_utf8_lossy(&pinned_bytes).into_owned();

    // Byte-level evidence: the port serves the pinned document verbatim
    // (the server includes it at compile time — sha equality is expected;
    // a mismatch with semantic equality would still be recorded here).
    let served_sha = Sha256::digest(served_response.body.as_slice());
    let pinned_sha = Sha256::digest(&pinned_bytes);
    let byte_identical = served_text == pinned_text;

    let served_value: Value =
        serde_json::from_str(&served_text).context("served openapi.json is not JSON")?;
    let pinned_value: Value =
        serde_json::from_str(&pinned_text).context("pinned openapi.json is not JSON")?;

    // -- semantic diff ------------------------------------------------------
    let reference = parity::openapi_diff::extract(&pinned_value);
    let candidate = parity::openapi_diff::extract(&served_value);
    let differences = parity::openapi_diff::diff(&reference, &candidate);

    // Whitelist: goldens/p10/openapi-whitelist.json = {"whitelist": [...]}
    let whitelist_path = golden_dir().join("openapi-whitelist.json");
    let mut whitelisted = Vec::new();
    if whitelist_path.is_file() {
        let doc: Value = serde_json::from_str(
            &std::fs::read_to_string(&whitelist_path)
                .with_context(|| format!("read {}", whitelist_path.display()))?,
        )
        .context("parse openapi-whitelist.json")?;
        whitelisted = doc
            .get("whitelist")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .or_else(|| v.get("difference").and_then(Value::as_str).map(str::to_string))
            })
            .collect();
    }
    let unexpected: Vec<&String> =
        differences.iter().filter(|d| !whitelisted.contains(d)).collect();
    let stale: Vec<&String> = whitelisted.iter().filter(|w| !differences.contains(w)).collect();

    // Evidence summary (golden-pinned).
    let summary = serde_json::json!({
        "pinned": {
            "path": pinned_path.display().to_string(),
            "sha256": format!("{pinned_sha:x}"),
        },
        "served": {
            "sha256": format!("{served_sha:x}"),
            "byte_identical_to_pinned": byte_identical,
        },
        "paths": reference.path_methods.len(),
        "operations": reference.operations.len(),
        "schemas": reference.schema_names.len(),
        "differences": differences,
        "whitelist": whitelisted,
        "stale_whitelist_entries": stale,
    });
    std::fs::create_dir_all(p10_common::artifacts())?;
    std::fs::write(
        p10_common::artifacts().join("openapi-summary.json"),
        format!("{}\n", serde_json::to_string_pretty(&summary)?),
    )?;
    parity::golden::assert_json_in(&golden_dir(), "openapi-summary.json", &summary)?;

    anyhow::ensure!(
        unexpected.is_empty(),
        "openapi semantic differences beyond the whitelist ({}): {unexpected:#?}",
        unexpected.len()
    );
    anyhow::ensure!(
        stale.is_empty(),
        "stale openapi whitelist entries (no longer produced — remove them): {stale:#?}"
    );

    eprintln!(
        "[p10-openapi] PASS — {} paths / {} operations / {} schemas, {} differences (whitelist: {}), \
         served document byte-identical to pinned: {byte_identical}",
        reference.path_methods.len(),
        reference.operations.len(),
        reference.schema_names.len(),
        differences.len(),
        whitelisted.len()
    );
    Ok(())
}
