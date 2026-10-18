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
