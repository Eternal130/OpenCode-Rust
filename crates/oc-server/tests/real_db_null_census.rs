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
/// our list does not know).
async fn probe_real_data(db_path: &Path) -> (String, String, usize) {
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
    (existing.0.clone(), existing.1.clone(), usize::try_from(existing.2).unwrap_or(0))
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
    let started = Instant::now();
    std::fs::copy(&staged, data.join("opencode.db")).expect("copy real DB into sandbox");
    eprintln!(
        "copied real DB ({} bytes) in {:?}",
        staged.metadata().unwrap().len(),
        started.elapsed()
    );

    redirect_env(&home, &log_file);

    // Pre-boot probe through the port's own database layer.
    let (busiest_session, real_directory, message_count) =
        probe_real_data(&data.join("opencode.db")).await;
    eprintln!(
        "busiest old session {busiest_session} ({message_count} messages) in {real_directory}"
    );
    assert!(message_count > 100, "the real DB copy must carry real conversations");

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
    assert_eq!(listed_count, 50, "a full default page of real sessions decodes");

    // -- one old v2 session's messages: 34k real message rows.
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
    assert!(messages.data.len() == 50, "first page full: got {}", messages.data.len());

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
