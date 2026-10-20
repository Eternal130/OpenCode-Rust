//! DB cluster S1 identity regressions (ledger DB-D1, DB-D2, DB-D3).
//!
//! - **DB-D1** `project_id_sha1`: project ids are `Hash.fast`
//!   (SHA-1, 40 hex) of `git-remote:<normalized>` / `directory:<abs>` —
//!   the v2 format. The port's `hash_fast` used SHA-256 (64 hex), which
//!   double-rows a repo when a v2 database is shared/imported
//!   (`packages/util/src/hash.ts:4-7`, `packages/core/src/project.ts:270`).
//! - **DB-D2** `project_activate`: `Project.activate`
//!   (`project.ts:241-255`) bumps `time_active` at most once per 60 s
//!   (keeping `time_updated` untouched) and publishes `project.updated`;
//!   the location lifecycle calls it at location start and on every
//!   durable session event for the location
//!   (`location/lifecycle.ts:34-45`).
//! - **DB-D3** `worktree_resolved_producer`: the production resolver
//!   publishes durable `worktree.resolved` events (with the markerless
//!   adoption scan, `project.ts:159-199`) instead of dropping them on
//!   `NoEvents` (`engine/mod.rs` assemble) — proven end-to-end through
//!   a booted engine's bus.
//!
//! Hermetic: sandbox dirs under /tmp/opencode, no network, `git2` local
//! repositories only.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::StreamExt as _;
use oc_core::project::{
    hash_fast, MemoryProjectStore, NoEvents, ProjectEvents, ProjectInfo, ProjectRow, ProjectStore,
    Resolver, WorktreeResolved,
};

/// Test-side twin of the crate-private `config::resolve_path`
/// (realpath when it exists, lexical absolutization otherwise).
fn resolve_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn tempdir(tag: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("oc-core-db-id-{tag}-"))
        .tempdir_in("/tmp/opencode")
        .expect("sandbox under /tmp/opencode")
}

fn init_repo(dir: &Path, remote: Option<&str>) {
    let repo = git2::Repository::init(dir).expect("git init");
    let mut config = repo.config().expect("config");
    config.set_str("user.email", "test@opencode.test").expect("email");
    config.set_str("user.name", "Test").expect("name");
    if let Some(remote) = remote {
        repo.remote("origin", remote).expect("remote");
    }
    let signature = git2::Signature::now("Test", "test@opencode.test").expect("sig");
    std::fs::write(dir.join("README.md"), "readme").expect("write");
    let mut index = repo.index().expect("index");
    index.add_path(Path::new("README.md")).expect("add");
    let tree_id = index.write_tree().expect("tree");
    let tree = repo.find_tree(tree_id).expect("find");
    repo.commit(Some("HEAD"), &signature, &signature, "root", &tree, &[]).expect("commit");
}

// ---------------------------------------------------------------------------
// DB-D1 — project id format (SHA-1, 40 hex)
// ---------------------------------------------------------------------------

/// SHA-1("git-remote:github.com/owner/repo") — the v2 remote identity.
const OWNER_REPO_SHA1: &str = "b00635c341dc9efd562030331b5ef18a77e1ac56";

#[tokio::test]
async fn project_id_sha1() {
    let dir = tempdir("d1");
    init_repo(dir.path(), Some("git@github.com:owner/repo.git"));

    let resolver = Resolver::new(Arc::new(MemoryProjectStore::new()), Arc::new(NoEvents));
    let project = resolver.resolve(dir.path()).await;

    // v2 `Hash.fast` = sha1 hex (40 chars), NOT sha256 (64 chars).
    assert_eq!(project.id.len(), 40, "project id must be 40-hex SHA-1, got {}", project.id);
    assert!(
        project.id.chars().all(|c| c.is_ascii_hexdigit()),
        "project id must be hex: {}",
        project.id
    );
    assert_eq!(project.id, OWNER_REPO_SHA1, "remote identity must match v2 Hash.fast");

    // Markerless directories hash `directory:<abs>` through the same
    // SHA-1 (`project.ts:270` → `Hash.fast`).
    let plain = tempdir("d1-plain");
    let markerless = resolver.resolve(plain.path()).await;
    let expected = {
        use sha1::Digest as _;
        let input = format!("directory:{}", resolve_path(plain.path()).display());
        let digest = sha1::Sha1::digest(input.as_bytes());
        let mut out = String::with_capacity(40);
        for byte in digest {
            use std::fmt::Write as _;
            let _ = write!(out, "{byte:02x}");
        }
        out
    };
    assert_eq!(markerless.id.len(), 40);
    assert_eq!(markerless.id, expected, "markerless identity must be sha1(directory:<abs>)");
}

// ---------------------------------------------------------------------------
// DB-D2 — Project.activate (rate-limited time_active + project.updated)
// ---------------------------------------------------------------------------

async fn project_db(tag: &str) -> oc_core::database::Database {
    let dir = tempdir(tag);
    oc_core::database::Database::open_file(dir.path().join("opencode.db")).await.expect("open db")
}

async fn project_time(db: &oc_core::database::Database, id: &str) -> (i64, i64) {
    let id = id.to_string();
    db.transaction(move |tx| {
        Ok(tx
            .conn()
            .query_row(
                "SELECT time_active, time_updated FROM project WHERE id = ?1",
                rusqlite::params![id],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .expect("project row"))
    })
    .await
    .expect("tx")
}

#[tokio::test]
async fn project_activate() {
    let db = project_db("d2").await;
    let store = oc_core::project::SqliteProjectStore::new(db.clone());
    let id = hash_fast("git-remote:github.com/owner/repo");
    store
        .upsert_project(&ProjectRow {
            id: id.clone(),
            canonical: PathBuf::from("/repo"),
            vcs: Some("git".into()),
        })
        .await;
    // Pin the row's clocks so the bump is observable.
    let seed_id = id.clone();
    db.transaction(move |tx| {
        tx.conn()
            .execute(
                "UPDATE project SET time_active = 0, time_updated = 111 WHERE id = ?1",
                rusqlite::params![seed_id],
            )
            .expect("seed clocks");
        Ok(())
    })
    .await
    .expect("seed tx");
    let before = project_time(&db, &id).await;
    assert_eq!(before, (0, 111));

    // First activation bumps time_active, keeps time_updated, and
    // returns the row for the project.updated announcement.
    let row = store.activate(&id).await.expect("activated");
    assert!(row.time_active >= before.0, "time_active bumped");
    let after = project_time(&db, &id).await;
    assert_eq!(after.0, row.time_active, "returned row matches the stored bump");
    assert_eq!(after.1, 111, "time_updated is NOT bumped by activity (project.ts:251-253)");

    // The 60 s rate limit suppresses repeat activations: no write, no row.
    let suppressed = store.activate(&id).await;
    assert!(suppressed.is_none(), "second activate inside the interval is suppressed");
    let unchanged = project_time(&db, &id).await;
    assert_eq!(unchanged, after, "no further write while rate-limited");

    // Unknown project: no row, no write.
    assert!(store.activate("does-not-exist").await.is_none());

    // Past the interval the activation fires again (test interval 0 ms).
    let store = oc_core::project::SqliteProjectStore::new_with_activate_interval(db.clone(), 0);
    let again = store.activate(&id).await.expect("activated past interval");
    assert!(again.time_active >= unchanged.0);
    let renewed = project_time(&db, &id).await;
    assert_eq!(renewed.0, again.time_active);
    assert_eq!(renewed.1, 111, "time_updated still untouched");
}

/// The production wiring: assembling a location activates its project
/// (`location-lifecycle.ts:34`) and the bump announces `project.updated`
/// on the engine bus.
#[tokio::test]
async fn project_activate_location_lifecycle_wiring() {
    let dir = tempdir("d2-engine");
    init_repo(dir.path(), None);
    std::fs::create_dir_all(dir.path().join("home")).expect("home");
    let engine = oc_core::engine::Engine::boot(oc_core::engine::EngineOptions {
        activity_sweeper: false,
        ..oc_core::engine::EngineOptions::new(dir.path().join("data"), dir.path().join("home"))
    })
    .await;
    let location = oc_core::location::LocationRef::new(dir.path().to_path_buf(), None);
    let mut updates = engine.bus.subscribe_types(&["project.updated"]);

    let services = engine.location(location).await.expect("assemble");
    let (time_active, time_updated) = project_time(&engine.db, &services.project.id).await;
    assert!(time_active > 0, "location start activated the project (time_active={time_active})");
    assert!(time_updated > 0, "row created with fresh timestamps (time_updated={time_updated})");

    // The activate announcement landed on the bus with the full Info
    // payload (fromRow: id/canonical/time...).
    let event = tokio::time::timeout(std::time::Duration::from_secs(5), updates.next())
        .await
        .expect("event within 5s")
        .expect("stream open");
    assert_eq!(event.type_, "project.updated");
    assert_eq!(event.data["id"], services.project.id);
    assert_eq!(
        event.data["canonical"].as_str(),
        Some(services.project.canonical.to_string_lossy().as_ref()),
    );
    assert!(event.data["time"]["active"].as_i64().unwrap_or(0) > 0);
    assert!(event.data.get("name").is_none(), "optional members are absent, not null");
}

// ---------------------------------------------------------------------------
// DB-D3 — worktree.resolved producer + adopted scan
// ---------------------------------------------------------------------------

/// Events recorder shared by the producer tests.
#[derive(Default)]
struct Recorder {
    resolved: std::sync::Mutex<Vec<WorktreeResolved>>,
    updated: std::sync::Mutex<Vec<ProjectInfo>>,
}

#[async_trait::async_trait]
impl ProjectEvents for Recorder {
    async fn worktree_resolved(&self, event: &WorktreeResolved) {
        self.resolved.lock().unwrap().push(event.clone());
    }
    async fn project_updated(&self, row: &ProjectInfo) {
        self.updated.lock().unwrap().push(row.clone());
    }
}

#[tokio::test]
async fn worktree_resolved_producer() {
    let db = project_db("d3").await;
    let dir = tempdir("d3-repo");
    init_repo(dir.path(), Some("git@github.com:owner/repo.git"));

    // A markerless project row nested under the repository (vcs NULL,
    // worktree under <repo>/...): resolving the repository adopts it
    // (project.ts:175-192 — the nearest marker from the candidate
    // resolves to the newly resolved directory).
    let nested = dir.path().join("nested");
    std::fs::create_dir_all(&nested).expect("nested dir");
    let markerless_id = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef".to_string();
    let store = oc_core::project::SqliteProjectStore::new(db.clone());
    store
        .upsert_project(&ProjectRow {
            id: markerless_id.clone(),
            canonical: nested.clone(),
            vcs: None,
        })
        .await;

    let events = Arc::new(Recorder::default());
    let resolver = Resolver::new(Arc::new(store), events.clone());
    let project = resolver.resolve(dir.path()).await;
    let expected_id = hash_fast("git-remote:github.com/owner/repo");
    assert_eq!(project.id, expected_id);

    // The durable fact (directory → project) is announced with the
    // adoption, exactly once for the canonical directory.
    {
        let guard = events.resolved.lock().unwrap();
        assert_eq!(guard.len(), 1, "one worktree.resolved for the canonical directory");
        assert_eq!(guard[0].project_id, expected_id);
        assert_eq!(guard[0].directory, resolve_path(dir.path()));
        assert_eq!(guard[0].previous, "global");
        assert_eq!(
            guard[0].adopted.as_deref(),
            Some(vec![markerless_id.clone()].as_slice()),
            "the markerless nested project is adopted"
        );
    }

    // Repeated resolution announces nothing new.
    let _ = resolver.resolve(dir.path()).await;
    assert_eq!(events.resolved.lock().unwrap().len(), 1);
}

/// A nested markerless project that owns a nearer repository of its own
/// (a `.git` between it and the outer directory) is NOT adopted.
#[tokio::test]
async fn worktree_resolved_adoption_requires_nearest_marker() {
    let db = project_db("d3b").await;
    let dir = tempdir("d3b-repo");
    init_repo(dir.path(), Some("git@github.com:owner/outer.git"));

    let nested = dir.path().join("inner-repo");
    init_repo(&nested, Some("git@github.com:owner/inner.git"));
    let markerless_id = "cafebabecafebabecafebabecafebabecafebabe".to_string();
    let store = oc_core::project::SqliteProjectStore::new(db);
    store
        .upsert_project(&ProjectRow { id: markerless_id, canonical: nested.clone(), vcs: None })
        .await;

    let events = Arc::new(Recorder::default());
    let resolver = Resolver::new(Arc::new(store), events.clone());
    let project = resolver.resolve(dir.path()).await;
    assert_ne!(project.id, "global");

    let guard = events.resolved.lock().unwrap();
    assert_eq!(guard.len(), 1);
    assert!(
        guard[0].adopted.is_none(),
        "a candidate with its own nearer repository is not adopted"
    );
}

/// End-to-end through a booted engine: assembling a location in a git
/// repository produces a durable `worktree.resolved` event on the bus.
#[tokio::test]
async fn worktree_resolved_engine_bus_wiring() {
    let dir = tempdir("d3-engine");
    init_repo(dir.path(), Some("git@github.com:owner/repo.git"));
    std::fs::create_dir_all(dir.path().join("home")).expect("home");
    let engine = oc_core::engine::Engine::boot(oc_core::engine::EngineOptions {
        activity_sweeper: false,
        ..oc_core::engine::EngineOptions::new(dir.path().join("data"), dir.path().join("home"))
    })
    .await;
    let mut resolved_events = engine.bus.subscribe_types(&["worktree.resolved"]);
    let location = oc_core::location::LocationRef::new(dir.path().to_path_buf(), None);
    let services = engine.location(location).await.expect("assemble");
    assert_eq!(services.project.id, hash_fast("git-remote:github.com/owner/repo"));

    let event = tokio::time::timeout(std::time::Duration::from_secs(5), resolved_events.next())
        .await
        .expect("event within 5s")
        .expect("stream open");
    assert_eq!(event.type_, "worktree.resolved");
    assert_eq!(event.data["projectID"], services.project.id);
    assert_eq!(
        event.data["directory"].as_str(),
        Some(resolve_path(dir.path()).to_string_lossy().as_ref()),
    );
    assert_eq!(event.data["previous"], "global");
    // Durable: the event row landed in the log (aggregate = projectID).
    assert!(event.durable.is_some(), "worktree.resolved is a durable event");
}
