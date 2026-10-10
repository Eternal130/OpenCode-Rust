//! Phase-10 DB interop with **v2-written NULL-heavy rows**: one shared data
//! home, three sequential boots — reference v2.0.24 writes, the Rust port
//! re-opens and continues, the reference re-opens again after the port.
//!
//! The D3 column class (`session_v2.model` / `.agent` / `.parent_id` NULL
//! cells) is exercised with REAL v2-written rows in both directions:
//!
//! - **Phase A (reference writes)** — via the API on the booted reference:
//!   session A created with NO model (fresh → `model`/`agent` NULL),
//!   session B created with the fake model then SWITCHED (non-NULL `model`,
//!   second switch exercises the non-empty previous-model arm later),
//!   session C created with `parentID: A` (non-NULL parent; A stays
//!   parentless), plus a full v2 prompt→message exchange on B.
//! - **Phase B (Rust opens, 0 reruns, continues)** — the Rust server boots
//!   on the same `XDG_DATA_HOME`: `GET /api/session` must read ALL v2
//!   rows (the NULL-cell read paths), the `migration` journal must be
//!   untouched (48 rows, identical `id`+`time_completed` — a re-run would
//!   rewrite timestamps/insert), then the port CONTINUES: a new prompt on
//!   B through `FakeLlm`, a model switch on the v2-written row, and a prompt
//!   on the NULL-model session A.
//! - **Phase C (reference re-opens)** — v2 reads the port-written rows
//!   (session list, switched model column, both replies in message.list)
//!   and continues with a third prompt on the same DB.
//!
//! The 48-entry migration manifest is golden-pinned
//! (`goldens/p10/db-migration-manifest.json`).
//!
//! Opt-in: `#[ignore]`d AND `OC_P10=1` at runtime (needs bun + reference):
//!
//! ```text
//! OC_P10=1 cargo nextest run --run-ignored only -E 'test(p10_db_interop)'
//! ```

mod p10_common;

use std::path::{Path, PathBuf};

use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};

use p10_common::{bun_db_dump, bun_path, enabled, skip};
use parity::scenario::ScenarioHost;

/// The shared-home env overrides that point a spawned server (either side)
/// at the shared data root. `TMPDIR` stays per-sandbox.
fn shared_env(home: &Path) -> Vec<(&'static str, String)> {
    vec![
        // **Finding P10-DB1**: the reference build pins channel `stable` →
        // `opencode-stable.db` (database-path.ts channel scheme); the port's
        // `Engine::boot` hard-pins `data_root/opencode.db` regardless of
        // `OPENCODE_CHANNEL`/`OPENCODE_DISABLE_CHANNEL_DB` (path.rs implements
        // the scheme, but the server's engine path bypasses it). v2's runtime
        // `OPENCODE_DISABLE_CHANNEL_DB=1` switch selects the unsuffixed
        // `opencode.db` — the ONE filename both implementations open, so the
        // interop battery drives on it.
        ("OPENCODE_DISABLE_CHANNEL_DB", "1".to_owned()),
        ("HOME", home.display().to_string()),
        ("OPENCODE_TEST_HOME", home.display().to_string()),
        ("XDG_DATA_HOME", home.join(".local/share").display().to_string()),
        ("XDG_STATE_HOME", home.join(".local/state").display().to_string()),
        ("XDG_CONFIG_HOME", home.join(".config").display().to_string()),
        ("XDG_CACHE_HOME", home.join(".cache").display().to_string()),
    ]
}

fn spawn_ref_on(home: &Path) -> parity::reference::ReferenceSpawn<'static> {
    // Owned Strings would need a lifetime-typed struct; the spawn options
    // only borrow str slices, so build 'static ones from the shared home.
    let env = shared_env(home)
        .into_iter()
        .map(|(k, v)| (k, Box::leak(v.into_boxed_str()) as &'static str))
        .collect();
    parity::reference::ReferenceSpawn { extra_env: env, seed_files: Vec::new() }
}

fn spawn_rust_on(home: &Path, config: &Path) -> parity::rust_server::RustSpawn<'static> {
    let mut env = shared_env(home)
        .into_iter()
        .map(|(k, v)| (k, Box::leak(v.into_boxed_str()) as &'static str))
        .collect::<Vec<_>>();
    env.push((
        "OPENCODE_CONFIG",
        Box::leak(config.display().to_string().into_boxed_str()) as &'static str,
    ));
    parity::rust_server::RustSpawn { extra_env: env, seed_files: Vec::new() }
}

/// Authed JSON call with owned path/body (call-site sugar for the phases).
async fn api(
    base: &str,
    password: &str,
    method: &str,
    path: String,
    body: Option<String>,
) -> anyhow::Result<(u16, Value)> {
    p10_common::api(base, password, method, &path, body.as_deref()).await
}

/// The `(id, time_completed)` migration journal as a comparable value.
fn journal(dump: &Value) -> Value {
    dump["migrations"].clone()
}

fn session_row<'a>(dump: &'a Value, id: &str) -> &'a Value {
    dump["sessions"]
        .as_array()
        .expect("sessions array")
        .iter()
        .find(|row| row["id"] == json!(id))
        .unwrap_or_else(|| panic!("session {id} missing from dump"))
}

#[tokio::test]
#[ignore = "Phase-10 battery: opt in with OC_P10=1 (needs bun + reference)"]
#[allow(clippy::too_many_lines)]
async fn p10_db_interop_v2_written_null_rows() -> Result<()> {
    if !enabled() {
        skip("OC_P10 != 1");
        return Ok(());
    }
    if !p10_common::reference_runnable() {
        skip("bun or the reference checkout is unavailable");
        return Ok(());
    }
    if parity::rust_server::skip_if_no_cargo() {
        skip("cargo not found on PATH");
        return Ok(());
    }
    let bun = bun_path()?;

    // -- the shared home + project -----------------------------------------
    let root = PathBuf::from(format!("/tmp/opencode/p10-db-{}", std::process::id()));
    let home = root.join("home");
    let project = root.join("project");
    for dir in [
        home.join(".local/share"),
        home.join(".local/state"),
        home.join(".config"),
        home.join(".cache"),
        project.clone(),
    ] {
        std::fs::create_dir_all(&dir)?;
    }
    let fake = p10_common::ScriptedFakeLlm::start(vec![
        p10_common::text_script("Parity fake says hello."),
        p10_common::text_script("Parity fake says hello."),
        p10_common::text_script("Parity fake says hello."),
        p10_common::text_script("Parity fake says hello."),
    ])
    .await?;
    let config = p10_common::provider_config(&fake.base(), true);
    let config_path = project.join("opencode.json");
    std::fs::write(&config_path, &config)?;
    let project_dir = project.display().to_string();
    let db = home.join(".local/share/opencode/opencode.db");

    // ======================================================================
    // Phase A — the reference writes NULL-heavy rows
    // ======================================================================
    let reference = parity::reference::spawn_reference(spawn_ref_on(&home)).await?;
    eprintln!("[p10-db] A: reference ready at {}", reference.url());
    let (base, password) = (reference.url().to_owned(), reference.password().to_owned());

    let (status, row) = api(
        &base,
        &password,
        "POST",
        "/api/session".to_owned(),
        Some(
            json!({"title": "p10 A null model", "location": {"directory": project_dir}})
                .to_string(),
        ),
    )
    .await?;
    ensure!(status == 200, "create A: {status} {row}");
    let a = row["data"]["id"].as_str().context("A id").unwrap().to_owned();

    let (status, row) = api(
        &base,
        &password,
        "POST",
        "/api/session".to_owned(),
        Some(
            json!({
                "title": "p10 B switched",
                "location": {"directory": project_dir},
                "model": {"providerID": "parityfake", "id": "fake-model"}
            })
            .to_string(),
        ),
    )
    .await?;
    ensure!(status == 200, "create B: {status} {row}");
    let b = row["data"]["id"].as_str().context("B id").unwrap().to_owned();

    // switch B once (the row's model column becomes non-NULL)
    let (status, body) = api(
        &base,
        &password,
        "POST",
        format!("/api/session/{b}/model"),
        Some(json!({"model": {"providerID": "parityfake", "id": "fake-model-a"}}).to_string()),
    )
    .await?;
    ensure!(status == 204, "switch B: {status} {body}");

    let (status, row) = api(
        &base,
        &password,
        "POST",
        "/api/session".to_owned(),
        Some(
            json!({"title": "p10 C child", "location": {"directory": project_dir}, "parentID": a})
                .to_string(),
        ),
    )
    .await?;
    ensure!(status == 200, "create C: {status} {row}");
    let c = row["data"]["id"].as_str().context("C id").unwrap().to_owned();

    // a full v2 prompt→message exchange on B
    let (status, body) = api(
        &base,
        &password,
        "POST",
        format!("/api/session/{b}/prompt"),
        Some(json!({"text": "Hello from v2."}).to_string()),
    )
    .await?;
    ensure!(status == 200, "prompt B on v2: {status} {body}");
    p10_common::wait_api(
        &base,
        &password,
        "GET",
        &format!("/api/session/{b}/message"),
        None,
        |value| {
            value["data"]
                .as_array()
                .is_some_and(|rows| rows.iter().any(|m| m["type"] == json!("assistant")))
        },
        30,
        2,
    )
    .await
    .context("v2 reply never landed")?;

    reference.stop().await?;
    ensure!(db.is_file(), "no database at {} after phase A", db.display());
    let dump_a = bun_db_dump(&bun, &db)?;
    std::fs::write(
        p10_common::artifacts().join("db-dump-phase-a.json"),
        format!("{}\n", serde_json::to_string_pretty(&dump_a)?),
    )?;

    // The D3 column class, on REAL v2 rows:
    let row_a = session_row(&dump_a, &a);
    ensure!(row_a["model"].is_null(), "A model not NULL: {row_a:?}");
    ensure!(row_a["agent"].is_null(), "A agent not NULL: {row_a:?}");
    ensure!(row_a["parent_id"].is_null(), "A parent not NULL: {row_a:?}");
    let row_b = session_row(&dump_a, &b);
    ensure!(!row_b["model"].is_null(), "B model unexpectedly NULL: {row_b:?}");
    let row_c = session_row(&dump_a, &c);
    ensure!(row_c["parent_id"] == json!(a), "C parent not A: {row_c:?}");
    let journal_a = journal(&dump_a);
    let ids_a: Vec<&str> = dump_a["migrations"]
        .as_array()
        .expect("migrations")
        .iter()
        .map(|m| m["id"].as_str().expect("id"))
        .collect();
    ensure!(ids_a.len() == 48, "expected 48 migration rows, got {}", ids_a.len());
    eprintln!(
        "[p10-db] A: v2 wrote A(NULL model/agent/parent), B(switched), C(child); 48 migrations"
    );

    // ======================================================================
    // Phase B — the Rust port opens (0 reruns) and continues
    // ======================================================================
    let port = parity::rust_server::spawn_rust(spawn_rust_on(&home, &config_path)).await?;
    eprintln!("[p10-db] B: rust ready at {}", port.url());
    let (base, password) = (port.url().to_owned(), port.password().to_owned());

    // The NULL-cell read paths: all v2 rows list, A's row GETs with NULL model.
    let list = p10_common::wait_api(
        &base,
        &password,
        "GET",
        "/api/session",
        None,
        |value| {
            let rows = value["data"].as_array().cloned().unwrap_or_default();
            [&a, &b, &c].iter().all(|id| rows.iter().any(|row| row["id"] == json!((**id).clone())))
        },
        10,
        2,
    )
    .await
    .context("the port did not list all v2-written sessions")?;
    let _ = list;
    let (status, row) = api(&base, &password, "GET", format!("/api/session/{a}"), None).await?;
    ensure!(status == 200, "get A through the port: {status} {row}");
    ensure!(
        row["data"]["model"].is_null() || row["data"].get("model").is_none(),
        "A model should read as NULL through the port: {row}"
    );
    eprintln!("[p10-db] B: read A (NULL model/agent) + B + C through the port");

    // 0 reruns: the journal is byte-identical (ids AND completion times).
    let dump_mid = bun_db_dump(&bun, &db)?;
    ensure!(
        journal(&dump_mid) == journal_a,
        "the migration journal changed after the Rust open (re-run?): A={journal_a} B={}",
        journal(&dump_mid)
    );
    eprintln!("[p10-db] B: migration journal untouched (0 reruns)");

    // Continue: a second prompt on the v2-written session B.
    let (status, body) = api(
        &base,
        &password,
        "POST",
        format!("/api/session/{b}/prompt"),
        Some(json!({"text": "Hello from rust."}).to_string()),
    )
    .await?;
    ensure!(status == 200, "prompt B through the port: {status} {body}");
    p10_common::wait_api(
        &base,
        &password,
        "GET",
        &format!("/api/session/{b}/message"),
        None,
        |value| {
            value["data"].as_array().is_some_and(|rows| {
                rows.iter().filter(|m| m["type"] == json!("assistant")).count() >= 2
            })
        },
        30,
        2,
    )
    .await
    .context("the port's reply on the v2 session never landed")?;
    eprintln!("[p10-db] B: continued B with a new prompt through the port");

    // Switch B again — the non-empty previous-model arm on a REAL v2 row.
    let (status, body) = api(
        &base,
        &password,
        "POST",
        format!("/api/session/{b}/model"),
        Some(json!({"model": {"providerID": "parityfake", "id": "fake-model"}}).to_string()),
    )
    .await?;
    ensure!(status == 204, "switch B through the port: {status} {body}");
    let row = p10_common::wait_api(
        &base,
        &password,
        "GET",
        &format!("/api/session/{b}"),
        None,
        |value| value.pointer("/data/model/id").and_then(Value::as_str) == Some("fake-model"),
        10,
        1,
    )
    .await
    .context("B's row never showed the port-side switch")?;
    let _ = row;

    // Continue the NULL-model session A too (config-default model resolution).
    let (status, body) = api(
        &base,
        &password,
        "POST",
        format!("/api/session/{a}/prompt"),
        Some(json!({"text": "Hello on the null-model session."}).to_string()),
    )
    .await?;
    ensure!(status == 200, "prompt A through the port: {status} {body}");
    p10_common::wait_api(
        &base,
        &password,
        "GET",
        &format!("/api/session/{a}/message"),
        None,
        |value| {
            value["data"]
                .as_array()
                .is_some_and(|rows| rows.iter().any(|m| m["type"] == json!("assistant")))
        },
        30,
        2,
    )
    .await
    .context("prompting the NULL-model v2 session through the port never replied")?;
    eprintln!("[p10-db] B: prompted the NULL-model session A through the port");

    port.stop().await?;
    let dump_b = bun_db_dump(&bun, &db)?;
    std::fs::write(
        p10_common::artifacts().join("db-dump-phase-b.json"),
        format!("{}\n", serde_json::to_string_pretty(&dump_b)?),
    )?;
    ensure!(journal(&dump_b) == journal_a, "the journal changed after the port's writes");
    let row_b = session_row(&dump_b, &b);
    ensure!(!row_b["model"].is_null(), "B model NULL after the port switch: {row_b:?}");

    // ======================================================================
    // Phase C — the reference re-opens after the port
    // ======================================================================
    let reference = parity::reference::spawn_reference(spawn_ref_on(&home)).await?;
    eprintln!("[p10-db] C: reference re-opened at {}", reference.url());
    let (base, password) = (reference.url().to_owned(), reference.password().to_owned());

    let list = p10_common::wait_api(
        &base,
        &password,
        "GET",
        "/api/session",
        None,
        |value| {
            let rows = value["data"].as_array().cloned().unwrap_or_default();
            [&a, &b, &c].iter().all(|id| rows.iter().any(|row| row["id"] == json!((**id).clone())))
        },
        10,
        2,
    )
    .await
    .context("v2 did not list the sessions after the port's writes")?;
    let _ = list;
    // v2 reads the port-written model switch.
    let (status, row) = api(&base, &password, "GET", format!("/api/session/{b}"), None).await?;
    ensure!(status == 200, "v2 get B: {status} {row}");
    ensure!(
        row.pointer("/data/model/id").and_then(Value::as_str) == Some("fake-model"),
        "v2 did not read the port's model switch: {row}"
    );
    // v2 reads BOTH replies (its own + the port's).
    let (status, messages) =
        api(&base, &password, "GET", format!("/api/session/{b}/message"), None).await?;
    ensure!(status == 200, "v2 message.list B: {status} {messages}");
    let assistant_count = messages["data"]
        .as_array()
        .map_or(0, |rows| rows.iter().filter(|m| m["type"] == json!("assistant")).count());
    ensure!(assistant_count >= 2, "v2 saw {assistant_count} assistant messages (want >= 2)");
    // v2 continues on the same DB.
    let (status, body) = api(
        &base,
        &password,
        "POST",
        format!("/api/session/{b}/prompt"),
        Some(json!({"text": "Hello again from v2."}).to_string()),
    )
    .await?;
    ensure!(status == 200, "v2 re-prompt B: {status} {body}");
    p10_common::wait_api(
        &base,
        &password,
        "GET",
        &format!("/api/session/{b}/message"),
        None,
        |value| {
            value["data"].as_array().is_some_and(|rows| {
                rows.iter().filter(|m| m["type"] == json!("assistant")).count() >= 3
            })
        },
        30,
        2,
    )
    .await
    .context("v2's third reply never landed")?;
    reference.stop().await?;

    let dump_c = bun_db_dump(&bun, &db)?;
    std::fs::write(
        p10_common::artifacts().join("db-dump-phase-c.json"),
        format!("{}\n", serde_json::to_string_pretty(&dump_c)?),
    )?;
    ensure!(journal(&dump_c) == journal_a, "the journal changed after v2's re-open");

    // -- the golden-pinned migration manifest (ids only; times are stamps) --
    let manifest = json!({
        "count": ids_a.len(),
        "ids": ids_a,
    });
    parity::golden::assert_json_in(
        &p10_common::golden_dir(),
        "db-migration-manifest.json",
        &manifest,
    )?;

    eprintln!(
        "[p10-db] PASS — v2 wrote (A NULL model/agent, B switched, C child), port re-opened with 0 \
         reruns + continued both, v2 re-opened + read port writes + continued; journal stable at \
         48 across all three phases"
    );
    Ok(())
}
