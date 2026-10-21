//! The real-DB money test: boots the REAL `boot_and_serve` against a
//! sandbox copy of the user's REAL migrated v2 database (1.4G, 891
//! sessions, 34k messages, every census column NULL-heavy, physical
//! ALTER-order columns) and exercises the original 500 repro plus the
//! read paths — create session, list, old-session messages — with a
//! never-silent-500 assertion on the sandboxed server log.
//!
//! Source of truth: a pre-staged copy at `/tmp/opencode/dbfix/
//! real-opencode.db` (online-backup of `/root/.local/share/opencode-rust/
//! opencode.db`, owned by the build user — the original is root-only and
//! must never be touched by tests). The test copies the staged master
//! into its own sandbox, so repeated runs never mutate the master.
//! Skips (with a message, green) when the staged copy is absent — e.g.
//! on CI or a fresh box.
//!
//! Hermetic: sandboxed HOME/XDG/config (the one-way v2 migration resolves
//! into the sandbox), sandboxed log sink (`OPENCODE_LOG_FILE`), port 0,
//! no network, no port 49374.

use std::path::{Path, PathBuf};
use std::time::Instant;

use oc_server::boot::{boot_and_serve, BootOptions};
use oc_server::lifecycle::ServeOptions;

/// The staged, user-readable copy of the real migrated DB (see module
/// header); `OC_REAL_DB` relocates it for other machines.
fn staged_real_db() -> Option<PathBuf> {
    let path = match std::env::var_os("OC_REAL_DB") {
        Some(path) => path,
        None => PathBuf::from("/tmp/opencode/dbfix/real-opencode.db").into_os_string(),
    };
    let path = PathBuf::from(path);
    path.is_file().then_some(path)
}

/// Removes the sandbox however the test exits (1.4G copies must not
/// linger in /tmp).
struct SandboxDrop(PathBuf);
impl Drop for SandboxDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// GATE-DB-FIXTURE-WAL: copy the staged fixture WITH its `-wal`/`-shm`
/// sidecars next to the sandbox main file. The CI `runtime-gates` chain
/// harvests the v2-written library db after the reference (WAL mode,
/// `packages/core/src/database/sqlite.bun.ts:93`) was ended without a
/// checkpoint — schema+data can still live in the `-wal` sidecar, and a
/// main-file-only copy is a 4096-byte 0-table shell (CI 39519736380:
/// `no such table: migration` right in `probe_real_data`). A checkpointed
/// fixture (no sidecars) copies the main file alone, unchanged behavior.
fn copy_db_with_sidecars(staged: &Path, target: &Path) -> std::io::Result<()> {
    std::fs::copy(staged, target)?;
    let target_name = target.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_owned();
    let staged_name = staged.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_owned();
    for suffix in ["-wal", "-shm"] {
        let side = staged.with_file_name(format!("{staged_name}{suffix}"));
        if side.is_file() {
            std::fs::copy(&side, target.with_file_name(format!("{target_name}{suffix}")))?;
        }
    }
    Ok(())
}

/// Hermetic pin for `copy_db_with_sidecars` (GATE-DB-FIXTURE-WAL): the
/// sidecars must land next to the sandbox main file when the staged
/// fixture carries them, and must not be invented when it does not.
/// Pure file semantics — no database engine needed here; the WAL-replay
/// half of the pin (a copied sidecar set really serves the rows) lives
/// in `tools/parity/tests/p10_db_import.rs`
/// (`p10_db_import_wal_sidecar_copy_completes_the_fixture`), where
/// rusqlite is available.
#[test]
fn real_db_null_census_wal_sidecar_copy_lands_next_to_the_main_file() {
    let root = PathBuf::from(format!(
        "/tmp/opencode/oc-server-realdb-sidecar-pin-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let staged = root.join("staged");
    std::fs::create_dir_all(&staged).expect("staged dir");
    // A fixture harvested WITH live sidecars.
    std::fs::write(staged.join("v2-library.db"), b"main-bytes").expect("main");
    std::fs::write(staged.join("v2-library.db-wal"), b"wal-bytes").expect("wal");
    std::fs::write(staged.join("v2-library.db-shm"), b"shm-bytes").expect("shm");
    let sandbox = root.join("sandbox");
    std::fs::create_dir_all(&sandbox).expect("sandbox dir");
    copy_db_with_sidecars(&staged.join("v2-library.db"), &sandbox.join("opencode.db"))
        .expect("copy with sidecars");
    assert_eq!(std::fs::read(sandbox.join("opencode.db")).unwrap(), b"main-bytes");
    assert_eq!(std::fs::read(sandbox.join("opencode.db-wal")).unwrap(), b"wal-bytes");
    assert_eq!(std::fs::read(sandbox.join("opencode.db-shm")).unwrap(), b"shm-bytes");

    // A checkpointed fixture (sidecars gone, e.g. the staged
    // wal_checkpoint(TRUNCATE) in the CI harvest removed them): the main
    // file alone must copy, and no phantom sidecars may appear.
    let checkpointed = root.join("checkpointed");
    std::fs::create_dir_all(&checkpointed).expect("checkpointed dir");
    std::fs::write(checkpointed.join("v2-library.db"), b"main-bytes").expect("main alone");
    let clean = root.join("sandbox-clean");
    std::fs::create_dir_all(&clean).expect("clean dir");
    copy_db_with_sidecars(&checkpointed.join("v2-library.db"), &clean.join("opencode.db"))
        .expect("copy without sidecars");
    assert_eq!(std::fs::read(clean.join("opencode.db")).unwrap(), b"main-bytes");
    assert!(!clean.join("opencode.db-wal").exists());
    assert!(!clean.join("opencode.db-shm").exists());

    std::fs::remove_dir_all(&root).ok();
}

fn redirect_env(home: &Path, log_file: &Path) {
    std::env::set_var("HOME", home);
    std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
    std::env::set_var("XDG_CACHE_HOME", home.join(".cache"));
    std::env::set_var("XDG_DATA_HOME", home.join(".local").join("share"));
    std::env::set_var("XDG_STATE_HOME", home.join(".local").join("state"));
    std::env::set_var("OPENCODE_CONFIG_DIR", home.join(".config").join("opencode-rust"));
    std::env::set_var("OPENCODE_LOG_FILE", log_file);
    std::env::remove_var("OPENCODE_DB");
    std::env::remove_var("OPENCODE_CONFIG");
    std::env::remove_var("OPENCODE_DEBUG_ERRORS");
}

/// One (sessionID, directory) with the most messages, plus a project
/// directory that exists on this machine — read through the port's own
/// database layer (post-migration journal check doubles as a no-op
/// migration assertion against the real journal, which contains an id
/// our list does not know). Also returns how many session rows the
/// database itself holds for that directory: the exact-equality ground
/// truth the `directory` list filter serves (`store.rs:168`, page limit
/// `DEFAULT_LIMIT = 50` in `handlers/session.rs:36`) — the census
/// asserts the server returns every row the fixture actually has.
async fn probe_real_data(db_path: &Path) -> (String, String, usize, usize) {
    let db = oc_core::database::Database::open_file(db_path)
        .await
        .expect("open real-DB copy through the port's database layer");
    let report = db.apply_report().expect("migration report");
    assert!(
        report.applied.is_empty(),
        "real journal must satisfy every migration (unknown ids tolerated), got {:?}",
        report.applied
    );
    let rows: Vec<(String, String, i64)> = db
        .with_conn(|conn| {
            let mut statement = conn
                .prepare(
                    "SELECT s.id, s.directory, count(m.id) AS n \
                     FROM session_v2 s JOIN session_message m ON m.session_id = s.id \
                     GROUP BY s.id, s.directory ORDER BY n DESC LIMIT 20",
                )
                .expect("prepare probe");
            let rows = statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?))
                })
                .expect("query probe")
                .collect::<Result<Vec<_>, _>>()
                .expect("decode probe");
            Ok(rows)
        })
        .await
        .expect("probe");
    let existing = rows
        .iter()
        .find(|(_, directory, _)| Path::new(directory).is_dir())
        .expect("at least one probed session directory exists on this machine");
    let directory_for_count = existing.1.clone();
    let in_directory = db
        .with_conn(move |conn| {
            Ok(conn.query_row(
                "SELECT count(*) FROM session_v2 WHERE directory = ?1",
                [&directory_for_count],
                |row| row.get::<_, i64>(0),
            )?)
        })
        .await
        .expect("count directory sessions");
    (
        existing.0.clone(),
        existing.1.clone(),
        usize::try_from(existing.2).unwrap_or(0),
        usize::try_from(in_directory).unwrap_or(0),
    )
}

/// Recreate fixture-referenced session directories that died with the
/// staging sandbox. The CI `runtime-gates` chain feeds this census a
/// v2-written fixture harvested from `/tmp/opencode/p10-db-<pid>/...`;
/// those pid-scoped directories are dead by the time the §12.3 job runs,
/// and the port resolves a dead location to `LocationNotFoundError` 404
/// (`oc-server/src/handlers/session.rs` services → `BootError::
/// MissingDirectory`) — which used to panic the probe at the
/// "at least one probed session directory exists" expect. Only paths
/// under `/tmp/opencode/` are ever created: a REAL library fixture
/// (projects under `$HOME`) is left untouched — its directories either
/// exist (unchanged behavior) or the row keeps being skipped. Twin of
/// `p10_common::recreate_missing_fixture_dirs` (this test lives in
/// oc-server, not the parity crate).
async fn revive_fixture_directories(db_path: &Path) -> usize {
    // Same layer the probe uses (oc_core's Database + with_conn); a bare
    // rusqlite open would need a dependency oc-server does not carry.
    let Ok(db) = oc_core::database::Database::open_file(db_path).await else {
        return 0;
    };
    let Ok(rows) = db
        .with_conn(|conn| {
            let mut statement = conn.prepare("SELECT DISTINCT directory FROM session_v2")?;
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .flatten()
                .collect::<Vec<_>>();
            Ok(rows)
        })
        .await
    else {
        return 0;
    };
    let mut created = 0;
    for directory in rows {
        let path = Path::new(&directory);
        if path.is_dir() || !path.starts_with("/tmp/opencode/") {
            continue;
        }
        if std::fs::create_dir_all(path).is_ok() {
            created += 1;
        }
    }
    created
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // one sequential end-to-end acceptance scenario
async fn real_migrated_db_serves_create_list_and_messages_without_500s() {
    let Some(staged) = staged_real_db() else {
        eprintln!("staged real DB copy absent; skipping (stage one with an online backup)");
        return;
    };

    let root = PathBuf::from(format!("/tmp/opencode/oc-server-realdb-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    let data = root.join("data");
    let project = root.join("project"); // neutral boot location: no config, no plugins
    let log_file = root.join("server.log");
    for dir in [&home, &data, &project] {
        std::fs::create_dir_all(dir).expect("sandbox dir");
    }
    let _sandbox = SandboxDrop(root.clone());

    // The working copy: the staged master is never mutated by the test.
    // GATE-DB-FIXTURE-WAL: WITH the staged -wal/-shm sidecars when the
    // fixture carries them (a killed-WAL harvest keeps schema+data in
    // the sidecar — a main-file-only copy is a 0-table shell).
    let started = Instant::now();
    copy_db_with_sidecars(&staged, &data.join("opencode.db"))
        .expect("copy real DB (with sidecars) into sandbox");
    eprintln!(
        "copied real DB ({} bytes) in {:?}",
        staged.metadata().unwrap().len(),
        started.elapsed()
    );

    redirect_env(&home, &log_file);

    // A harvested fixture (CI's runtime-gates chain) references dead
    // pid-scoped staging directories; recreate them (guarded to
    // /tmp/opencode/) BEFORE the probe picks the target directory.
    let revived = revive_fixture_directories(&data.join("opencode.db")).await;
    if revived > 0 {
        eprintln!("revived {revived} dead fixture session directories");
    }

    // Pre-boot probe through the port's own database layer.
    let (busiest_session, real_directory, message_count, directory_sessions) =
        probe_real_data(&data.join("opencode.db")).await;
    eprintln!(
        "busiest old session {busiest_session} ({message_count} messages) in {real_directory} \
         ({directory_sessions} sessions in that directory)"
    );
    assert!(message_count > 0, "the real DB copy must carry real conversations");

    let options = BootOptions {
        serve: ServeOptions { hostname: std::net::IpAddr::from([127, 0, 0, 1]), port: Some(0) },
        password: None,
        cors_origins: Vec::new(),
        version: "2.0.24".to_owned(),
        data_root: data,
        home,
        config_location: Some(project),
    };
    let handle = boot_and_serve(&options).await.expect("boot and serve on the real DB copy");
    let client = oc_client::Client::new(&format!("http://{}/", handle.addr)).expect("client");

    // -- THE ORIGINAL 500 REPRO: POST /api/session on the real migrated DB.
    // (Pre-fix: `sqlite error: Invalid column type Null at index: 35,
    // name: idle_outcome` → UnknownError 500 on every create.)
    let created = client
        .session()
        .create(&oc_client::types::SessionCreateBody {
            location: Some(oc_client::types::LocationPublicRef {
                directory: real_directory.clone(),
            }),
            title: Some("repro".to_owned()),
            ..Default::default()
        })
        .await
        .expect("session.create on the real migrated DB must not 500");
    let created_id = created.id.clone();
    eprintln!("created session {created_id} against the real DB");
    assert_eq!(created.title.as_deref(), Some("repro"), "created row round-trips: {created:?}");

    // -- list: decodes real rows full of census NULLs (the endpoint pages
    // at 50 by default — a full page proves bulk decode over real data).
    // GATE-DB-NULLROWS calibration: the expected count is the database's
    // OWN row count for the directory (`directory = ?` equality, the
    // store's filter at oc-core/src/session/store.rs:168), capped by the
    // page limit (DEFAULT_LIMIT = 50, handlers/session.rs:36). For the
    // original 1.4G library fixture this is exactly the old assertion
    // (891 sessions in the project directory → a full page of 50); for
    // the CI-harvested p10 fixture (a handful of v2-written rows) it is
    // that fixture's whole population — and STRICTLY stronger as a
    // parity property: the server must serve every row the db holds, no
    // drops, no synthetic padding.
    let listed = client
        .session()
        .list(&oc_client::types::SessionListQuery {
            directory: Some(real_directory.clone()),
            ..Default::default()
        })
        .await
        .expect("session.list over the real migrated rows must not 500");
    let listed_count = listed.data.len();
    eprintln!("listed {listed_count} sessions for {real_directory}");
    // +1: the census itself just created a session in this directory
    // (persisted through the projector before create responded) — for
    // the 1.4G library this stays a full page of 50; for a small fixture
    // it also pins that the port's own create landed and is served.
    assert_eq!(
        listed_count,
        (directory_sessions + 1).min(50),
        "the directory's session page must serve exactly the rows the db holds"
    );

    // -- one old v2 session's messages: the first page is full to its
    // limit or exhausts the session (same calibration; the busiest real
    // library session has 34k messages → still a full page of 50).
    let messages = client
        .message()
        .list(
            &busiest_session,
            &oc_client::types::SessionMessageListQuery {
                limit: Some("50".to_owned()),
                ..Default::default()
            },
        )
        .await
        .expect("old v2 session messages must load");
    eprintln!("loaded {} messages of {busiest_session}", messages.data.len());
    assert_eq!(
        messages.data.len(),
        message_count.min(50),
        "the busiest session's first page must serve exactly the rows the db holds"
    );

    // -- old session info: the row read behind every session route.
    let info =
        client.session().get(&busiest_session).await.expect("old v2 session info must decode");
    assert_eq!(info.id, busiest_session);

    // -- never-silent-500s: the sandbox sink must hold NO 5xx envelope
    // lines for this run (and the mechanism itself is unit-tested in
    // errors.rs — a 500 would land here AND on stderr).
    let log = std::fs::read_to_string(&log_file).unwrap_or_default();
    assert!(!log.contains("[api-error] 5"), "server 500s occurred: {log}");

    handle.shutdown_and_join().await.expect("shutdown");
}
