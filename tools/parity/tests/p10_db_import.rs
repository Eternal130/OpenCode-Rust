//! Gate 2b — real v2-library-copy import (PARITY-LOOP.md §8.3 Gate 2,
//! B0-2 skeleton).
//!
//! The complement to `p10_db_interop.rs` (which writes NULL-heavy rows with
//! the harness's own boots): this gate imports a **copy of a real v2
//! library database** into a private lane sandbox and proves the Rust port
//! (then the reference) opens it without damage:
//!
//! - **sandbox copy helper** — `PARITY_V2_DB_FIXTURE` points at a real v2
//!   `opencode.db` (plus optional `-wal`/`-shm` siblings); it is COPIED
//!   into `<sandbox>/home/.local/share/opencode/opencode.db`. The real
//!   data dirs (`~/.local/share/opencode`, `/root/.cache/opencode`, …) are
//!   never touched.
//! - **open-with-journal check** — after the Rust boot the migration
//!   journal contains every pre-existing `(id, time_completed)` pair
//!   unchanged (0 re-runs); catch-up rows for an older fixture are allowed
//!   and recorded, and the reference re-open after that keeps the journal
//!   stable again.
//! - **project-id no-double-rows** — the `project` table gains no
//!   duplicate ids and no rewritten rows across both re-opens.
//! - **continue-session smoke** — the fixture's newest session lists, GETs,
//!   and its messages read stably through the port (read-path continue);
//!   the write-path prompt continue is wired behind
//!   `PARITY_V2_DB_FIXTURE_PROMPT=1` because it depends on fixture content
//!   (the session's model/providers) this skeleton cannot assume.
//!
//! **No v2 library fixture exists in-tree yet.** The fixture env is the
//! explicit gate: unset under `OC_GATE_STRICT=1` this FAILS with a
//! named-missing message (`parity::gate`); unset in daily runs it skips
//! with a note like every other environment prerequisite.
//!
//! Opt-in: `#[ignore]`d AND `OC_P10=1` at runtime:
//!
//! ```text
//! OC_P10=1 PARITY_V2_DB_FIXTURE=/path/to/real-v2-opencode.db \
//!   cargo nextest run --run-ignored only -E 'test(p10_db_import)'
//! ```
//!
//! Artifact: `/tmp/opencode/p10-artifacts/gate2b-db-import.json`.

mod p10_common;

use std::path::{Path, PathBuf};

use anyhow::{ensure, Context, Result};
use serde_json::Value;

use p10_common::{bun_path, enabled, skip};
use parity::scenario::ScenarioHost;

/// The env naming the real v2 library database to import a copy of.
const FIXTURE_ENV: &str = "PARITY_V2_DB_FIXTURE";
/// Opt-in write-path continue (prompt on the fixture's newest session).
const PROMPT_ENV: &str = "PARITY_V2_DB_FIXTURE_PROMPT";

/// The lane sandbox root (acceptance: under `/tmp/opencode/`, pid-scoped).
fn sandbox_root() -> PathBuf {
    PathBuf::from(format!("/tmp/opencode/lane-b0-gates-{}-dbimport", std::process::id()))
}

/// Copy `fixture` (+ `-wal`/`-shm` siblings) into the sandbox data dir,
/// returning the sandbox DB path. The fixture itself is only ever read.
fn copy_fixture_into_sandbox(fixture: &Path, sandbox: &Path) -> Result<PathBuf> {
    let data = sandbox.join("home/.local/share/opencode");
    std::fs::create_dir_all(&data)?;
    let target = data.join("opencode.db");
    std::fs::copy(fixture, &target)
        .with_context(|| format!("copy fixture {} -> {}", fixture.display(), target.display()))?;
    let stem = fixture.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_owned();
    for suffix in ["-wal", "-shm"] {
        let side = fixture.with_file_name(format!("{stem}{suffix}"));
        if side.is_file() {
            std::fs::copy(&side, target.with_file_name(format!("opencode.db{suffix}")))
                .with_context(|| format!("copy fixture sidecar {}", side.display()))?;
        }
    }
    Ok(target)
}

/// Dump `{migrations, sessions, projects}` from a sandbox DB copy via
/// `bun:sqlite` (same trick as `p10_common::bun_db_dump`, extended with the
/// `project` table for the no-double-rows assertion; kept local so the
/// shared helper's evidence shape stays untouched).
fn dump(bun: &Path, db: &Path) -> Result<Value> {
    let scratch = p10_common::artifacts().join(format!("gate2b-copy-{}.db", std::process::id()));
    std::fs::copy(db, &scratch).with_context(|| format!("copy {}", db.display()))?;
    let stem = scratch.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
    for suffix in ["-wal", "-shm"] {
        let side = db.with_file_name(format!(
            "{}{suffix}",
            db.file_name().and_then(|n| n.to_str()).unwrap_or_default()
        ));
        if side.is_file() {
            let _ = std::fs::copy(&side, scratch.with_file_name(format!("{stem}{suffix}")));
        }
    }
    let script = r#"
import { Database } from "bun:sqlite";
const db = new Database(process.argv[process.argv.length - 1]);
const migrations = db
  .query("SELECT id, time_completed FROM migration ORDER BY id")
  .all();
const sessions = db
  .query("SELECT id, parent_id, agent, model FROM session_v2 ORDER BY time_created, id")
  .all();
const projects = db
  .query("SELECT id, worktree, vcs FROM project ORDER BY id")
  .all();
console.log(JSON.stringify({ migrations, sessions, projects }));
"#;
    let output = std::process::Command::new(bun)
        .arg("-e")
        .arg(script)
        .arg(&scratch)
        .output()
        .with_context(|| format!("bun -e sqlite dump on {}", scratch.display()))?;
    ensure!(
        output.status.success(),
        "bun sqlite dump failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(text.trim()).with_context(|| format!("parse bun dump: {text}"))
}

/// Assert every `(id, time_completed)` pair from `before` survives into
/// `after` unchanged — migration catch-up rows may be appended, re-runs
/// (rewritten timestamps / duplicated ids) may not.
fn journal_no_reruns(label: &str, before: &Value, after: &Value) -> Result<usize> {
    let pairs = |value: &Value| -> Vec<(String, Value)> {
        value["migrations"]
            .as_array()
            .map(|rows| {
                rows.iter()
                    .map(|row| {
                        (
                            row["id"].as_str().unwrap_or_default().to_owned(),
                            row["time_completed"].clone(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let old = pairs(before);
    let new = pairs(after);
    for (id, completed) in &old {
        let survived = new.iter().any(|(nid, ncompleted)| nid == id && ncompleted == completed);
        ensure!(
            survived,
            "{label}: migration {id} vanished or re-ran (time_completed changed) — journal was \
             {} rows, now {}",
            old.len(),
            new.len()
        );
    }
    ensure!(new.len() >= old.len(), "{label}: journal shrank ({} -> {})", old.len(), new.len());
    Ok(new.len() - old.len())
}

/// Assert the `project` table gained no duplicate ids and kept every
/// pre-existing row byte-identical.
fn project_no_double_rows(label: &str, before: &Value, after: &Value) -> Result<()> {
    let rows = |value: &Value| -> Vec<(String, Value)> {
        value["projects"]
            .as_array()
            .map(|rows| {
                rows.iter()
                    .map(|row| (row["id"].as_str().unwrap_or_default().to_owned(), row.clone()))
                    .collect()
            })
            .unwrap_or_default()
    };
    let old = rows(before);
    let new = rows(after);
    let mut seen = std::collections::HashSet::new();
    for (id, _) in &new {
        ensure!(seen.insert(id.clone()), "{label}: project id {id} appears in multiple rows");
    }
    for (id, row) in &old {
        let survived = new.iter().any(|(nid, nrow)| nid == id && nrow == row);
        ensure!(survived, "{label}: pre-existing project row {id} changed or vanished: {row}");
    }
    Ok(())
}

/// The shared-home env overrides (the `p10_db_interop` unsuffixed-file
/// lane: both sides on `opencode.db`).
fn shared_env(home: &Path) -> Vec<(&'static str, String)> {
    vec![
        ("OPENCODE_DISABLE_CHANNEL_DB", "1".to_owned()),
        ("HOME", home.display().to_string()),
        ("OPENCODE_TEST_HOME", home.display().to_string()),
        ("XDG_DATA_HOME", home.join(".local/share").display().to_string()),
        ("XDG_STATE_HOME", home.join(".local/state").display().to_string()),
        ("XDG_CONFIG_HOME", home.join(".config").display().to_string()),
        ("XDG_CACHE_HOME", home.join(".cache").display().to_string()),
    ]
}

#[tokio::test]
#[ignore = "Phase-10 gate: opt in with OC_P10=1 + PARITY_V2_DB_FIXTURE (a real v2 library db)"]
async fn p10_db_import_real_library_copy() -> Result<()> {
    if !enabled() {
        skip("OC_P10 != 1");
        return Ok(());
    }
    if parity::rust_server::skip_if_no_cargo() {
        return Ok(());
    }
    if p10_common::bun_path().is_err()
        && parity::gate::skip_or_fail("bun not found (set PARITY_BUN)")
    {
        return Ok(());
    }
    // The fixture is a named environment prerequisite: strict mode FAILS
    // here (no self-skip) until a real v2 library copy is provided.
    let fixture = if let Ok(path) = std::env::var(FIXTURE_ENV) {
        PathBuf::from(path)
    } else if parity::gate::skip_or_fail(&format!(
        "{FIXTURE_ENV} not set: Gate 2b imports a copy of a REAL v2 library database \
         (none exists in-tree yet; copy one out of a real installation and point \
         {FIXTURE_ENV} at it — see tools/parity/README.md 'Gate 2b')"
    )) {
        return Ok(());
    } else {
        unreachable!("skip_or_fail panics under strict mode");
    };
    ensure!(fixture.is_file(), "{FIXTURE_ENV}={} does not point at a file", fixture.display());
    run(&fixture, std::env::var(PROMPT_ENV).ok().as_deref() == Some("1"))
        .await
        .with_context(|| format!("import fixture {}", fixture.display()))
}

#[allow(clippy::too_many_lines)]
async fn run(fixture: &Path, prompt_continue: bool) -> Result<()> {
    let bun = bun_path()?;
    let sandbox = sandbox_root();
    for dir in [
        sandbox.join("home/.local/share"),
        sandbox.join("home/.local/state"),
        sandbox.join("home/.config"),
        sandbox.join("home/.cache"),
        sandbox.join("project"),
    ] {
        std::fs::create_dir_all(&dir)?;
    }
    let db = copy_fixture_into_sandbox(fixture, &sandbox)?;
    eprintln!("[gate2b] fixture copied to {}", db.display());

    // Phase 0 — the untouched import baseline.
    let baseline = dump(&bun, &db).context("dump the imported baseline")?;
    let baseline_sessions = baseline["sessions"].as_array().map_or(0, Vec::len);
    ensure!(
        baseline_sessions > 0,
        "fixture has no session_v2 rows — not a usable v2 library copy: {baseline:?}"
    );
    eprintln!(
        "[gate2b] baseline: {} sessions, {} migrations",
        baseline_sessions,
        baseline["migrations"].as_array().map_or(0, Vec::len)
    );

    // Phase B — the Rust port opens the imported copy and continues.
    let fake = p10_common::ScriptedFakeLlm::start(vec![p10_common::text_script(
        "Parity fake says hello.",
    )])
    .await?;
    let config = p10_common::provider_config(&fake.base(), false);
    let config_path = sandbox.join("project/opencode.json");
    std::fs::write(&config_path, &config)?;
    let mut env: Vec<(&'static str, &'static str)> = shared_env(&sandbox.join("home"))
        .into_iter()
        .map(|(k, v)| (k, Box::leak(v.into_boxed_str()) as &'static str))
        .collect();
    env.push((
        "OPENCODE_CONFIG",
        Box::leak(config_path.display().to_string().into_boxed_str()) as &'static str,
    ));
    let port = parity::rust_server::spawn_rust(parity::rust_server::RustSpawn {
        extra_env: env,
        seed_files: Vec::new(),
    })
    .await?;
    eprintln!("[gate2b] B: rust opened the import at {}", port.url());

    let list = p10_common::wait_api(
        port.url(),
        port.password(),
        "GET",
        "/api/session",
        None,
        |value| value["data"].as_array().is_some_and(|rows| rows.len() >= baseline_sessions),
        15,
        2,
    )
    .await
    .context("the port did not list the imported sessions")?;
    let newest = list["data"]
        .as_array()
        .and_then(|rows| rows.first().cloned())
        .context("imported session list empty")?;
    let newest_id = newest["id"].as_str().context("newest session id")?.to_owned();
    eprintln!(
        "[gate2b] B: newest imported session {newest_id} (title {:?})",
        newest["title"].as_str().unwrap_or_default()
    );

    // continue-session smoke (read path): row + messages, stable across
    // two GETs.
    let (status, row) = p10_common::api(
        port.url(),
        port.password(),
        "GET",
        &format!("/api/session/{newest_id}"),
        None,
    )
    .await?;
    ensure!(status == 200, "get newest through the port: {status} {row}");
    let (status, messages) = p10_common::api(
        port.url(),
        port.password(),
        "GET",
        &format!("/api/session/{newest_id}/message"),
        None,
    )
    .await?;
    ensure!(status == 200, "message list through the port: {status} {messages}");
    let message_count = messages["data"].as_array().map_or(0, Vec::len);
    let (_, messages_again) = p10_common::api(
        port.url(),
        port.password(),
        "GET",
        &format!("/api/session/{newest_id}/message"),
        None,
    )
    .await?;
    ensure!(
        messages_again["data"].as_array().map_or(0, Vec::len) == message_count,
        "imported message list is not stable across reads"
    );
    eprintln!(
        "[gate2b] B: continue smoke — {message_count} imported messages stable through the port"
    );

    // Optional write-path continue (fixture-content dependent; opt-in).
    let mut prompt_continued = false;
    if prompt_continue {
        let (status, body) = p10_common::api(
            port.url(),
            port.password(),
            "POST",
            &format!("/api/session/{newest_id}/prompt"),
            Some(r#"{"text":"Gate 2b continue smoke."}"#),
        )
        .await?;
        ensure!(status == 200, "prompt on the imported session: {status} {body}");
        p10_common::wait_api(
            port.url(),
            port.password(),
            "GET",
            &format!("/api/session/{newest_id}/message"),
            None,
            |value| value["data"].as_array().map_or(0, Vec::len) > message_count,
            30,
            2,
        )
        .await
        .context("the continue prompt on the imported session never replied")?;
        prompt_continued = true;
    }

    port.stop().await?;
    let after_b = dump(&bun, &db).context("dump after the Rust open")?;
    let caught_up = journal_no_reruns("rust open", &baseline, &after_b)?;
    project_no_double_rows("rust open", &baseline, &after_b)?;
    let sessions_b = after_b["sessions"].as_array().cloned().unwrap_or_default();
    ensure!(
        sessions_b.len() >= baseline_sessions,
        "sessions shrank after the Rust open ({} -> {})",
        baseline_sessions,
        sessions_b.len()
    );
    eprintln!("[gate2b] B: journal 0 re-runs (+{caught_up} catch-up rows), project ids unique");

    // Phase C — the reference re-opens the same import.
    let env: Vec<(&'static str, &'static str)> = shared_env(&sandbox.join("home"))
        .into_iter()
        .map(|(k, v)| (k, Box::leak(v.into_boxed_str()) as &'static str))
        .collect();
    let reference = parity::reference::spawn_reference(parity::reference::ReferenceSpawn {
        extra_env: env,
        seed_files: Vec::new(),
    })
    .await?;
    eprintln!("[gate2b] C: reference re-opened the import at {}", reference.url());
    let seen = p10_common::wait_api(
        reference.url(),
        reference.password(),
        "GET",
        "/api/session",
        None,
        |value| value["data"].as_array().is_some_and(|rows| rows.len() >= baseline_sessions),
        15,
        2,
    )
    .await
    .context("the reference did not list the imported sessions after the port's open")?;
    let _ = seen;
    reference.stop().await?;
    let after_c = dump(&bun, &db).context("dump after the reference re-open")?;
    journal_no_reruns("reference re-open", &after_b, &after_c)?;
    project_no_double_rows("reference re-open", &after_b, &after_c)?;

    let report = serde_json::json!({
        "gate": "gate2b-db-import",
        "fixture": fixture.display().to_string(),
        "baseline": {
            "sessions": baseline_sessions,
            "migrations": baseline["migrations"].as_array().map_or(0, Vec::len),
        },
        "rust_open": {
            "journal_catch_up_rows": caught_up,
            "project_ids": after_b["projects"].as_array().map_or(0, Vec::len),
            "continue_smoke": if prompt_continued { "prompt continue ok" } else { "not requested" },
            "newest_session": newest_id,
        },
        "reference_reopen": "ok",
        "checks": [
            "journal 0 re-runs across both opens (catch-up allowed, recorded)",
            "project id no-double-rows across both opens",
            "continue-session read smoke (stable message list)",
        ],
        "structured_gaps": {
            "write_path_continue": format!(
                "prompt continue wired behind {PROMPT_ENV}=1 (depends on fixture model/providers)"
            ),
            "table_dump_diff": "normalized full-table dump diff deferred until a fixture is \
             pinned (paths in a real library copy do not normalize under the sandbox root)",
        },
    });
    std::fs::create_dir_all(p10_common::artifacts())?;
    std::fs::write(
        p10_common::artifacts().join("gate2b-db-import.json"),
        format!("{}\n", serde_json::to_string_pretty(&report)?),
    )?;
    eprintln!("[gate2b] PASS — import opened by port + reference, journal/project checks clean");
    Ok(())
}

// ---------------------------------------------------------------------------
// DB-D12 — v1 content migration (v1-migration.bun.ts port)
// ---------------------------------------------------------------------------

/// The v1 legacy schema, exactly as the reference's own workflow tests
/// create it after a fresh v2 bootstrap (packages/core/test/
/// v1-migration.test.ts `createLegacyTables`).
const LEGACY_DDL: &str = "
CREATE TABLE session (
  id text PRIMARY KEY,
  project_id text NOT NULL,
  workspace_id text,
  parent_id text,
  slug text NOT NULL,
  directory text NOT NULL,
  path text,
  title text NOT NULL,
  version text NOT NULL,
  share_url text,
  summary_additions integer,
  summary_deletions integer,
  summary_files integer,
  summary_diffs text,
  metadata text,
  cost real DEFAULT 0 NOT NULL,
  tokens_input integer DEFAULT 0 NOT NULL,
  tokens_output integer DEFAULT 0 NOT NULL,
  tokens_reasoning integer DEFAULT 0 NOT NULL,
  tokens_cache_read integer DEFAULT 0 NOT NULL,
  tokens_cache_write integer DEFAULT 0 NOT NULL,
  revert text,
  permission text,
  agent text,
  model text,
  time_created integer NOT NULL,
  time_updated integer NOT NULL,
  time_compacting integer,
  time_archived integer
);
CREATE TABLE message (
  id text PRIMARY KEY, session_id text NOT NULL,
  time_created integer NOT NULL, time_updated integer NOT NULL, data text NOT NULL
);
CREATE TABLE part (
  id text PRIMARY KEY, message_id text NOT NULL, session_id text NOT NULL,
  time_created integer NOT NULL, time_updated integer NOT NULL, data text NOT NULL
);
";

fn user_data(time: i64, overrides: &str) -> String {
    format!(
        r#"{{"role":"user","time":{{"created":{time}}},"agent":"build","model":{{"providerID":"provider","modelID":"model"}}{overrides}}}"#
    )
}

fn assistant_data(time: i64, parent: &str, cost: i64, tokens: [i64; 5], overrides: &str) -> String {
    format!(
        r#"{{"role":"assistant","time":{{"created":{time},"completed":{}}},"parentID":"{parent}","modelID":"model","providerID":"provider","mode":"build","agent":"build","path":{{"cwd":"/tmp/test","root":"/tmp/test"}},"cost":{cost},"tokens":{{"total":10,"input":{},"output":{},"reasoning":{},"cache":{{"read":{},"write":{}}}}}{overrides}}}"#,
        time + 5,
        tokens[0],
        tokens[1],
        tokens[2],
        tokens[3],
        tokens[4],
    )
}

async fn exec(db: &oc_core::database::Database, sql: &str) {
    let sql = sql.to_owned();
    db.transaction(move |tx| {
        tx.execute_batch(&sql)?;
        Ok(())
    })
    .await
    .expect("exec");
}

async fn one(db: &oc_core::database::Database, sql: &str) -> String {
    // Wrapped in cast-as-text so REAL/INTEGER columns stringify uniformly.
    let sql = format!("SELECT cast(({sql}) as text)");
    db.transaction(move |tx| Ok(tx.conn().query_row(&sql, [], |row| row.get::<_, String>(0))?))
        .await
        .expect("one")
}

/// DB-D12: the v1 session/message/part content lands in `session_v2` +
/// `session_message` with the reference transform's shapes (warnings,
/// aggregates, watermark, project reassignment, idempotency), and the
/// optional opencode-next.db import runs alongside.
#[tokio::test]
#[allow(clippy::too_many_lines, reason = "one DB-D12 scenario walk")]
async fn v1_content_migration() {
    use oc_core::database::v1_migration::{self, RunOptions, Status};

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sandbox =
        PathBuf::from(format!("/tmp/opencode/lane-db-d12-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&sandbox).expect("sandbox");
    let db =
        oc_core::database::Database::open_file(sandbox.join("opencode.db")).await.expect("open db");

    // A legacy database inside the fresh v2 schema (the reference workflow:
    // bootstrap, then createLegacyTables + fixture rows).
    exec(&db, LEGACY_DDL).await;
    exec(
        &db,
        &format!(
            r#"
INSERT INTO session (id, project_id, slug, directory, title, version, time_created, time_updated)
  VALUES ('ses_orphan', 'missing-project', 'orphan', '/tmp/orphan', 'Orphan', '1', 1, 2);
INSERT INTO session (id, project_id, slug, directory, title, version, time_created, time_updated)
  VALUES ('ses_a', 'global', 'a', '/tmp/test', 'Test', '1', 1, 2);
INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES
  ('msg_000000000001aaaaaaaaaaaaaa', 'ses_a', 10, 11, '{}'),
  ('msg_000000000002aaaaaaaaaaaaaa', 'ses_a', 20, 21, '{}'),
  ('msg_000000000003aaaaaaaaaaaaaa', 'ses_a', 30, 31, '{}'),
  ('msg_000000000004aaaaaaaaaaaaaa', 'ses_a', 31, 32, '{}'),
  ('msg_000000000005aaaaaaaaaaaaaa', 'ses_a', 40, 41, '{}'),
  ('msg_000000000006aaaaaaaaaaaaaa', 'ses_a', 41, 42, '{}'),
  ('msg_bad', 'ses_a', 50, 51, '{{'),
  ('msg_schema', 'ses_a', 51, 52, '{{"role":"user"}}');
INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES
  ('prt_u1', 'msg_000000000001aaaaaaaaaaaaaa', 'ses_a', 1, 2, '{{"type":"text","text":"first"}}'),
  ('prt_u2', 'msg_000000000001aaaaaaaaaaaaaa', 'ses_a', 1, 2, '{{"type":"text","text":"second"}}'),
  ('prt_u3', 'msg_000000000001aaaaaaaaaaaaaa', 'ses_a', 1, 2, '{{"type":"agent","name":"build","source":{{"value":"@build","start":2,"end":8}}}}'),
  ('prt_u4', 'msg_000000000001aaaaaaaaaaaaaa', 'ses_a', 1, 2, '{{"type":"file","mime":"text/plain","filename":"inline.txt","url":"data:text/plain,hello%20world"}}'),
  ('prt_u5', 'msg_000000000001aaaaaaaaaaaaaa', 'ses_a', 1, 2, '{{"type":"file","mime":"text/plain","url":"https://example.test/raw"}}'),
  ('prt_a1', 'msg_000000000002aaaaaaaaaaaaaa', 'ses_a', 1, 2, '{{"type":"text","text":"","metadata":{{"separator":true}}}}'),
  ('prt_a2', 'msg_000000000002aaaaaaaaaaaaaa', 'ses_a', 1, 2, '{{"type":"reasoning","text":"think","metadata":{{"provider":1}},"time":{{"start":21,"end":22}}}}'),
  ('prt_a3', 'msg_000000000002aaaaaaaaaaaaaa', 'ses_a', 1, 2, '{{"type":"step-start","snapshot":"snap_start"}}'),
  ('prt_a4', 'msg_000000000002aaaaaaaaaaaaaa', 'ses_a', 1, 2, '{{"type":"tool","callID":"call_read","tool":"read","state":{{"status":"completed","input":{{}},"output":"done","title":"Read","metadata":{{"result":true}},"time":{{"start":31,"end":32}}}}}}'),
  ('prt_a5', 'msg_000000000002aaaaaaaaaaaaaa', 'ses_a', 1, 2, '{{"type":"tool","callID":"call_pending","tool":"glob","state":{{"status":"pending","input":{{}},"raw":"{{}}"}}}}'),
  ('prt_c1', 'msg_000000000003aaaaaaaaaaaaaa', 'ses_a', 1, 2, '{{"type":"compaction","auto":false}}'),
  ('prt_c2', 'msg_000000000004aaaaaaaaaaaaaa', 'ses_a', 1, 2, '{{"type":"text","text":"summary"}}'),
  ('prt_n1', 'msg_000000000006aaaaaaaaaaaaaa', 'ses_a', 1, 2, '{{"type":"text","text":"later"}}'),
  ('prt_n2', 'msg_000000000006aaaaaaaaaaaaaa', 'ses_a', 1, 2, '{{"type":"tool","callID":"call_bash","tool":"bash","state":{{"status":"completed","input":{{}},"output":"ok","title":"bash","metadata":{{}},"time":{{"start":1,"end":2}}}}}}'),
  ('prt_x1', 'msg_000000000001aaaaaaaaaaaaaa', 'ses_a', 1, 2, '{{"type":"future","value":true}}'),
  ('prt_x2', 'msg_missing', 'ses_a', 1, 2, '{{"type":"text","text":"orphan"}}');
"#,
            user_data(10, ""),
            assistant_data(20, "msg_000000000001aaaaaaaaaaaaaa", 1, [2, 3, 4, 5, 6], r#","finish":"stop""#),
            user_data(30, ""),
            assistant_data(31, "msg_000000000003aaaaaaaaaaaaaa", 1, [2, 3, 4, 5, 6], r#","summary":true"#),
            user_data(40, ""),
            assistant_data(41, "msg_000000000005aaaaaaaaaaaaaa", 2, [3, 4, 5, 6, 7], ""),
        ),
    )
    .await;

    // Status: the legacy `session` table + no completed KV state → required.
    assert_eq!(v1_migration::status(&db).await, Status::Required);

    // transform_session parity spot-checks on the same fixture (pure path):
    // exact warning set/order and the deterministic synthetic-id derivation.
    {
        let messages = vec![
            v1_migration::SourceMessage {
                id: "msg_000000000001aaaaaaaaaaaaaa".into(),
                session_id: "ses_a".into(),
                time_created: 10,
                time_updated: 11,
                data: user_data(10, ""),
            },
            v1_migration::SourceMessage {
                id: "msg_bad".into(),
                session_id: "ses_a".into(),
                time_created: 50,
                time_updated: 51,
                data: "{".into(),
            },
        ];
        let parts = vec![
            v1_migration::SourcePart {
                id: "prt_1".into(),
                message_id: "msg_000000000001aaaaaaaaaaaaaa".into(),
                session_id: "ses_a".into(),
                time_created: 1,
                time_updated: 2,
                data: r#"{"type":"text","text":"first"}"#.into(),
            },
            v1_migration::SourcePart {
                id: "prt_2".into(),
                message_id: "msg_000000000001aaaaaaaaaaaaaa".into(),
                session_id: "ses_a".into(),
                time_created: 1,
                time_updated: 2,
                data: "{".into(),
            },
            v1_migration::SourcePart {
                id: "prt_3".into(),
                message_id: "msg_missing".into(),
                session_id: "ses_a".into(),
                time_created: 1,
                time_updated: 2,
                data: r#"{"type":"text","text":"orphan"}"#.into(),
            },
        ];
        let result = v1_migration::transform_session(
            &v1_migration::SourceSession { id: "ses_a".into(), agent: None, model: None },
            &messages,
            &parts,
        );
        assert_eq!(result.warnings.len(), 3, "{:?}", result.warnings);
        assert_eq!(result.warnings[0].reason, "invalid-message");
        assert_eq!(result.warnings[0].message_id.as_deref(), Some("msg_bad"));
        assert_eq!(result.warnings[1].reason, "invalid-part");
        assert!(result.warnings[1].observed_type.is_none(), "bad json has no type");
        assert_eq!(result.warnings[2].reason, "orphan-part");
        assert_eq!(result.warnings[2].message_id.as_deref(), Some("msg_missing"));

        // The reference's exact deterministic split id (BigInt base-62 of
        // sha256("v1-synthetic:...") behind a 16-char prefix).
        let mixed_user = v1_migration::SourceMessage {
            id: "msg_000000000004aaaaaaaaaaaaaa".into(),
            session_id: "ses_a".into(),
            time_created: 2,
            time_updated: 3,
            data: user_data(2, ""),
        };
        let synthetic = v1_migration::transform_session(
            &v1_migration::SourceSession { id: "ses_a".into(), agent: None, model: None },
            &[mixed_user],
            &[
                v1_migration::SourcePart {
                    id: "prt_1".into(),
                    message_id: "msg_000000000004aaaaaaaaaaaaaa".into(),
                    session_id: "ses_a".into(),
                    time_created: 1,
                    time_updated: 2,
                    data: r#"{"type":"text","text":"hello"}"#.into(),
                },
                v1_migration::SourcePart {
                    id: "prt_2".into(),
                    message_id: "msg_000000000004aaaaaaaaaaaaaa".into(),
                    session_id: "ses_a".into(),
                    time_created: 1,
                    time_updated: 2,
                    data: r#"{"type":"text","text":"hidden","synthetic":true}"#.into(),
                },
            ],
        );
        assert_eq!(synthetic.messages.len(), 2, "user + split synthetic");
        assert_eq!(synthetic.messages[0].r#type, "user");
        assert_eq!(synthetic.messages[1].r#type, "synthetic");
        assert_eq!(
            synthetic.messages[1].id, "msg_000000000004ST20Jh98kGJtjL",
            "the reference's exact deterministic synthetic id"
        );
    }

    // Run: completes, idempotent, status flips.
    let options = RunOptions { in_memory: true, ..Default::default() };
    assert_eq!(v1_migration::run(&db, options.clone()).await.expect("run"), "completed");
    assert_eq!(v1_migration::status(&db).await, Status::Completed);
    assert_eq!(v1_migration::run(&db, options).await.expect("rerun"), "completed");

    // Project reassignment: the orphan session moved to `global`, whose
    // worktree is the data-root filesystem root ("/" on unix).
    assert_eq!(
        one(&db, "SELECT project_id FROM session_v2 WHERE id = 'ses_orphan'").await,
        "global"
    );
    assert_eq!(one(&db, "SELECT worktree FROM project WHERE id = 'global'").await, "/");
    assert_eq!(
        one(&db, "SELECT value FROM kv WHERE key = 'migration.v1-v2'").await,
        r#"{"phase":"completed"}"#
    );

    // The projection: contiguous seqs, exact types, the trailing
    // legacy-tool notice for the post-compaction `bash` call.
    let rows: Vec<(String, String, i64)> = db
        .transaction(|tx| {
            let mut statement = tx.conn().prepare(
                "SELECT id, type, seq FROM session_message WHERE session_id = 'ses_a' ORDER BY seq",
            )?;
            let rows = statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
        .expect("rows");
    let shape: Vec<(&str, i64)> = rows.iter().map(|(_, ty, seq)| (ty.as_str(), *seq)).collect();
    assert_eq!(
        shape,
        vec![
            ("user", 0),
            ("assistant", 1),
            ("compaction", 2),
            ("user", 3),
            ("assistant", 4),
            ("system", 5),
        ],
        "v1 → v2 projection sequence: {rows:?}"
    );
    assert!(rows[5].0.starts_with("msg_000000000006"), "system notice id prefix: {:?}", rows[5]);

    // The user row: joined text + inline file (base64) + agents + the
    // unavailable placeholder for the remote attachment.
    let user = serde_json::from_str::<Value>(
        &one(&db, "SELECT data FROM session_message WHERE id = 'msg_000000000001aaaaaaaaaaaaaa'")
            .await,
    )
    .expect("json");
    assert_eq!(user["agents"][0]["name"], "build");
    assert_eq!(user["agents"][0]["mention"]["text"], "@build");
    assert_eq!(user["files"][0]["data"], "aGVsbG8gd29ybGQ=");
    assert_eq!(user["files"][0]["name"], "inline.txt");
    assert_eq!(
        user["text"],
        "first\n\nsecond\n\n[Attachment unavailable after migration: https://example.test/raw (text/plain)]"
    );

    // The assistant row: reasoning/tool content, snapshot start, finish.
    let assistant = serde_json::from_str::<Value>(
        &one(&db, "SELECT data FROM session_message WHERE id = 'msg_000000000002aaaaaaaaaaaaaa'")
            .await,
    )
    .expect("json");
    assert_eq!(assistant["content"][0]["state"], serde_json::json!({"separator": true}));
    assert_eq!(
        assistant["content"][1]["time"],
        serde_json::json!({"created": 21, "completed": 22})
    );
    assert_eq!(assistant["content"][2]["state"]["status"], "completed");
    assert_eq!(assistant["content"][2]["state"]["content"][0]["text"], "done");
    assert_eq!(assistant["content"][3]["state"]["error"]["type"], "tool.interrupted");
    assert_eq!(assistant["snapshot"]["start"], "snap_start");
    assert_eq!(assistant["finish"], "stop");
    assert_eq!(
        assistant["tokens"],
        serde_json::json!({"input": 2, "output": 3, "reasoning": 4, "cache": {"read": 5, "write": 6}})
    );

    // The compaction row (paired summary collapsed in, manual reason).
    let compaction = serde_json::from_str::<Value>(
        &one(&db, "SELECT data FROM session_message WHERE id = 'msg_000000000003aaaaaaaaaaaaaa'")
            .await,
    )
    .expect("json");
    assert_eq!(compaction["status"], "completed");
    assert_eq!(compaction["reason"], "manual");
    assert_eq!(compaction["summary"], "summary");
    assert_eq!(compaction["recent"], "");

    // The system notice names the renamed tool called after the compaction.
    let notice = serde_json::from_str::<Value>(
        &one(
            &db,
            &format!(
                "SELECT data FROM session_message WHERE id = '{}' AND type = 'system'",
                rows[5].0
            ),
        )
        .await,
    )
    .expect("json");
    assert_eq!(
        notice["text"],
        "The available tools have changed.\n\nThe `bash` tool is now `shell` and must be called by that name."
    );

    // Aggregates: every assistant's usage (incl. the collapsed summary),
    // agent/model backfilled from the latest conversational user message.
    assert_eq!(
        one(&db, "SELECT cost FROM session_v2 WHERE id = 'ses_a'").await.parse::<f64>().unwrap(),
        4.0
    );
    assert_eq!(
        one(&db, "SELECT tokens_input FROM session_v2 WHERE id = 'ses_a'")
            .await
            .parse::<i64>()
            .unwrap(),
        7
    );
    assert_eq!(
        one(&db, "SELECT tokens_cache_write FROM session_v2 WHERE id = 'ses_a'")
            .await
            .parse::<i64>()
            .unwrap(),
        19
    );
    assert_eq!(one(&db, "SELECT agent FROM session_v2 WHERE id = 'ses_a'").await, "build");
    assert_eq!(
        one(&db, "SELECT model FROM session_v2 WHERE id = 'ses_a'").await,
        r#"{"id":"model","providerID":"provider","variant":"default"}"#
    );

    // The bus watermark: the last projected seq, unowned.
    assert_eq!(
        one(&db, "SELECT seq FROM event_sequence WHERE aggregate_id = 'ses_a'")
            .await
            .parse::<i64>()
            .unwrap(),
        5
    );
    assert_eq!(
        one(
            &db,
            "SELECT coalesce(owner_id, '<null>') FROM event_sequence WHERE aggregate_id = 'ses_a'"
        )
        .await,
        "<null>"
    );

    // Idempotency: the rerun did not duplicate rows.
    assert_eq!(
        one(&db, "SELECT COUNT(*) FROM session_message WHERE session_id = 'ses_a'")
            .await
            .parse::<i64>()
            .unwrap(),
        6
    );

    // --- opencode-next.db import (previous V2 database) ------------------
    // The pre-split next format: `project`/`session`/`session_message`
    // tables (the reference test's exact fixture, apostrophes included).
    let next_path = sandbox.join("opencode-next.db");
    {
        let next = rusqlite::Connection::open(&next_path).expect("next db");
        next.execute_batch(
            r#"
CREATE TABLE project (
  id text PRIMARY KEY, worktree text NOT NULL, vcs text, name text, icon_url text, icon_url_override text,
  icon_color text, time_created integer NOT NULL, time_updated integer NOT NULL, time_initialized integer,
  sandboxes text NOT NULL, commands text
);
CREATE TABLE session (
  id text PRIMARY KEY, project_id text NOT NULL, workspace_id text, parent_id text, fork_session_id text,
  fork_boundary text, slug text NOT NULL, directory text NOT NULL, path text, title text, version text NOT NULL,
  share_url text, summary_additions integer, summary_deletions integer, summary_files integer, summary_diffs text,
  metadata text, cost real DEFAULT 0 NOT NULL, tokens_input integer DEFAULT 0 NOT NULL,
  tokens_output integer DEFAULT 0 NOT NULL, tokens_reasoning integer DEFAULT 0 NOT NULL,
  tokens_cache_read integer DEFAULT 0 NOT NULL, tokens_cache_write integer DEFAULT 0 NOT NULL, revert text,
  permission text, agent text, model text, time_created integer NOT NULL, time_updated integer NOT NULL,
  time_compacting integer, time_archived integer, time_suspended integer
);
CREATE TABLE session_message (
  id text PRIMARY KEY, session_id text NOT NULL, type text NOT NULL, seq integer NOT NULL,
  time_created integer NOT NULL, time_updated integer NOT NULL, data text NOT NULL
);
INSERT INTO project VALUES (
  'next-project', '/tmp/next', 'git', 'Source project', NULL, NULL, NULL, 1, 2, NULL, '[]', NULL
);
INSERT INTO session (id, project_id, slug, directory, title, version, agent, model, time_created, time_updated)
  VALUES ('ses_next', 'next-project', 'next', '/tmp/next', 'Imported', '2', 'build',
          '{"id":"model","providerID":"provider"}', 10, 20),
         ('ses_orphan2', 'missing-project', 'orphan2', '/tmp/orphan2', 'Orphan', '2', NULL, NULL, 12, 22);
INSERT INTO session_message VALUES
  ('msg_next', 'ses_next', 'user', 4, 12, 13, '{"text":"from next''s history","time":{"created":12}}');
"#,
        )
        .expect("next fixture");
    }

    // A fresh legacy-carrying database, with a pre-existing row the import
    // must not clobber (same id in both stores).
    let db2_root = sandbox.join("second");
    std::fs::create_dir_all(&db2_root).expect("second sandbox");
    let db2 = oc_core::database::Database::open_file(db2_root.join("opencode.db"))
        .await
        .expect("open db2");
    exec(&db2, LEGACY_DDL).await;
    exec(
        &db2,
        r#"
INSERT INTO session (id, project_id, slug, directory, title, version, time_created, time_updated)
  VALUES ('ses_legacy', 'global', 'legacy', '/tmp/legacy', 'Legacy', '1', 1, 2);
INSERT INTO project (id, worktree, name, time_created, time_updated, sandboxes)
  VALUES ('next-project', '/tmp/current', 'Current project', 1, 2, '[]');
INSERT INTO session_v2 (id, project_id, slug, directory, title, version, time_created, time_updated)
  VALUES ('ses_existing', 'next-project', 'current-existing', '/tmp/current', 'Current existing', '2', 1, 2);
INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data)
  VALUES ('msg_current_existing', 'ses_existing', 'user', 0, 1, 2, '{"text":"current","time":{"created":1}}');
"#,
    )
    .await;
    let options = RunOptions { next_database_path: Some(next_path), ..Default::default() };
    assert_eq!(v1_migration::run(&db2, options).await.expect("run2"), "completed");

    // The import landed the new session verbatim (apostrophes intact) with
    // its event-sequence watermark…
    assert_eq!(one(&db2, "SELECT title FROM session_v2 WHERE id = 'ses_next'").await, "Imported");
    assert_eq!(
        one(&db2, "SELECT data FROM session_message WHERE id = 'msg_next'").await,
        r#"{"text":"from next's history","time":{"created":12}}"#
    );
    assert_eq!(
        one(&db2, "SELECT seq FROM event_sequence WHERE aggregate_id = 'ses_next'")
            .await
            .parse::<i64>()
            .unwrap(),
        4
    );
    // …skipped the pre-existing session (incumbent wins)…
    assert_eq!(
        one(&db2, "SELECT title FROM session_v2 WHERE id = 'ses_existing'").await,
        "Current existing"
    );
    assert_eq!(
        one(&db2, "SELECT COUNT(*) FROM session_message WHERE session_id = 'ses_existing' AND id = 'msg_current_existing'").await.parse::<i64>().unwrap(),
        1
    );
    // …kept the incumbent project row (INSERT OR IGNORE)…
    assert_eq!(
        one(&db2, "SELECT name FROM project WHERE id = 'next-project'").await,
        "Current project"
    );
    // …reassigned the orphan to global and completed the legacy walk.
    assert_eq!(
        one(&db2, "SELECT project_id FROM session_v2 WHERE id = 'ses_orphan2'").await,
        "global"
    );
    assert_eq!(
        one(&db2, "SELECT project_id FROM session_v2 WHERE id = 'ses_legacy'").await,
        "global"
    );
    assert_eq!(
        one(&db2, "SELECT value FROM kv WHERE key = 'migration.v1-v2'").await,
        r#"{"phase":"completed"}"#
    );
}
