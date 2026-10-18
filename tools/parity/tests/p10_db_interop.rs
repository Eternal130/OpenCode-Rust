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
        // **P10-DB1 — FIXED**: both sides now resolve the database through
        // the channel scheme (the engine uses `database::path`, the channel
        // comes from the one workspace source). This mode keeps v2's
        // `OPENCODE_DISABLE_CHANNEL_DB=1` runtime switch — the unsuffixed
        // `opencode.db` both implementations share — as the battery's
        // regression lane; the stable-channel lane below exercises the
        // channel-suffixed file for real.
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

/// `Result` form (B0-2): a missing row FAILS the test as an `Err` so the
/// known-red wrapper can categorize it — the pre-B0-2 form panicked, which
/// bypassed categorization (same assertion, different failure channel).
fn session_row<'a>(dump: &'a Value, id: &str) -> Result<&'a Value> {
    dump["sessions"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("sessions missing from dump: {dump:?}"))?
        .iter()
        .find(|row| row["id"] == json!(id))
        .ok_or_else(|| anyhow::anyhow!("session {id} missing from dump"))
}

#[tokio::test]
#[ignore = "Phase-10 battery: opt in with OC_P10=1 (needs bun + reference)"]
async fn p10_db_interop_v2_written_null_rows() -> Result<()> {
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
    // Known-red wrapper (B0-2, §8.5): this lane is a registered red — the
    // failure stays a failure, only the message is categorized so
    // known-red-summary.py can tell KNOWN-RED from NEW-RED.
    if let Err(error) = run_v2_written_null_rows().await {
        return Err(p10_common::known_red("GATE-DB-NULLROWS", "Batch 1B").categorize(error));
    }
    Ok(())
}

// The debug-formatting allow keeps the extracted body byte-identical to the
// pre-B0-2 #[tokio::test] fn (clippy only sees through the plain-async form).
#[allow(clippy::too_many_lines, clippy::unnecessary_debug_formatting)]
async fn run_v2_written_null_rows() -> Result<()> {
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
    let row_a = session_row(&dump_a, &a)?;
    ensure!(row_a["model"].is_null(), "A model not NULL: {row_a:?}");
    ensure!(row_a["agent"].is_null(), "A agent not NULL: {row_a:?}");
    ensure!(row_a["parent_id"].is_null(), "A parent not NULL: {row_a:?}");
    let row_b = session_row(&dump_a, &b)?;
    ensure!(!row_b["model"].is_null(), "B model unexpectedly NULL: {row_b:?}");
    let row_c = session_row(&dump_a, &c)?;
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
    let row_b = session_row(&dump_b, &b)?;
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

// ---------------------------------------------------------------------------
// P10-DB1 stable-channel lane: with the channel scheme wired into
// `Engine::boot`, a channel-matched Rust boot opens the SAME
// channel-suffixed file the (stable-pinned) reference build opens —
// `opencode-stable.db` — with no DISABLE switch on either side.
// ---------------------------------------------------------------------------

#[tokio::test]
#[ignore = "Phase-10 battery: opt in with OC_P10=1 (needs bun + reference)"]
async fn p10_db_interop_stable_channel_same_file() -> Result<()> {
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
    // Known-red wrapper (B0-2, §8.5): registered red GATE-DB-STABLE — see
    // docs/parity-baseline.md §2.1; categorized, never skipped.
    if let Err(error) = run_stable_channel_same_file().await {
        return Err(p10_common::known_red("GATE-DB-STABLE", "Batch 1B").categorize(error));
    }
    Ok(())
}

// Same: extracted verbatim from the #[tokio::test] wrapper (B0-2).
#[allow(clippy::too_many_lines, clippy::unnecessary_debug_formatting)]
async fn run_stable_channel_same_file() -> Result<()> {
    let bun = bun_path()?;

    let root = PathBuf::from(format!("/tmp/opencode/p10-db-stable-{}", std::process::id()));
    let home = root.join("home");
    let data = home.join(".local/share/opencode");
    std::fs::create_dir_all(&data)?;
    // The stable-channel env: the reference build's baked channel define
    // IS `stable` (spawn pins OPENCODE_CHANNEL="stable"), so it needs no
    // runtime switch; the Rust engine reads OPENCODE_CHANNEL.
    let env = |home: &Path| {
        vec![
            ("HOME", home.display().to_string()),
            ("OPENCODE_TEST_HOME", home.display().to_string()),
            ("XDG_DATA_HOME", home.join(".local/share").display().to_string()),
            ("XDG_STATE_HOME", home.join(".local/state").display().to_string()),
            ("XDG_CONFIG_HOME", home.join(".config").display().to_string()),
            ("XDG_CACHE_HOME", home.join(".cache").display().to_string()),
        ]
    };
    let static_env: Vec<(&'static str, &'static str)> = env(&home)
        .into_iter()
        .map(|(k, v)| (k, Box::leak(v.into_boxed_str()) as &'static str))
        .collect();
    let mut rust_env = static_env.clone();
    rust_env.push(("OPENCODE_CHANNEL", "stable"));
    let stable_db = data.join("opencode-stable.db");

    // Phase A — the reference writes through its channel-suffixed file.
    let reference = parity::reference::spawn_reference(parity::reference::ReferenceSpawn {
        extra_env: static_env.clone(),
        seed_files: Vec::new(),
    })
    .await?;
    let (base, password) = (reference.url().to_owned(), reference.password().to_owned());
    let (status, row) = api(
        &base,
        &password,
        "POST",
        "/api/session".to_owned(),
        Some(json!({"title": "p10 stable A written by v2"}).to_string()),
    )
    .await?;
    ensure!(status == 200, "stable create A: {status} {row}");
    let a = row["data"]["id"].as_str().context("A id").unwrap().to_owned();
    reference.stop().await?;
    ensure!(stable_db.is_file(), "reference did not open {stable_db:?}");
    ensure!(
        !data.join("opencode.db").exists(),
        "an unsuffixed opencode.db appeared next to the stable file"
    );

    // Phase B — the Rust boot with OPENCODE_CHANNEL=stable opens the SAME
    // file: v2's row is visible, a port-written row lands beside it, and
    // the migration journal did not re-run.
    let port = parity::rust_server::spawn_rust(parity::rust_server::RustSpawn {
        extra_env: rust_env,
        seed_files: Vec::new(),
    })
    .await?;
    let (base, password) = (port.url().to_owned(), port.password().to_owned());
    let seen = p10_common::wait_api(
        &base,
        &password,
        "GET",
        "/api/session",
        None,
        |value| {
            value["data"]
                .as_array()
                .is_some_and(|rows| rows.iter().any(|row| row["id"] == json!(a)))
        },
        10,
        2,
    )
    .await
    .context("the stable-channel Rust boot did not see v2's session (wrong file?)")?;
    let _ = seen;
    let (status, row) = api(
        &base,
        &password,
        "POST",
        "/api/session".to_owned(),
        Some(json!({"title": "p10 stable B written by the port"}).to_string()),
    )
    .await?;
    ensure!(status == 200, "stable create B through the port: {status} {row}");
    let b = row["data"]["id"].as_str().context("B id").unwrap().to_owned();
    port.stop().await?;
    ensure!(stable_db.is_file(), "the port did not keep the stable file");
    let dump = bun_db_dump(&bun, &stable_db)?;
    let ids: Vec<&str> = dump["migrations"]
        .as_array()
        .map_or(vec![], |rows| rows.iter().map(|m| m["id"].as_str().unwrap_or("")).collect());
    ensure!(ids.len() == 48, "stable file journal has {} rows (want 48)", ids.len());
    // The dump's session projection is (id, parent_id, agent, model) —
    // `session_row` panics when the id is absent, so this is the
    // port-write-landed assertion.
    let _ = session_row(&dump, &b)?;

    // Phase C — the reference re-opens the same file and reads the port's row.
    let reference = parity::reference::spawn_reference(parity::reference::ReferenceSpawn {
        extra_env: static_env,
        seed_files: Vec::new(),
    })
    .await?;
    let (base, password) = (reference.url().to_owned(), reference.password().to_owned());
    let seen = p10_common::wait_api(
        &base,
        &password,
        "GET",
        "/api/session",
        None,
        |value| {
            value["data"].as_array().is_some_and(|rows| {
                rows.iter().any(|row| row["id"] == json!(a))
                    && rows.iter().any(|row| row["id"] == json!(b))
            })
        },
        10,
        2,
    )
    .await
    .context("v2 did not see both rows in the stable file after the port's write")?;
    let _ = seen;
    reference.stop().await?;

    eprintln!("[p10-db-stable] PASS — v2 and a OPENCODE_CHANNEL=stable Rust boot inter-operated on opencode-stable.db (A by v2, B by the port, journal 48)");
    Ok(())
}
