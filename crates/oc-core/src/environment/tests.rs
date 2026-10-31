//! Tests for the environment execution layer (G-1 port).

use std::path::PathBuf;
use std::sync::Arc;

use crate::environment::files::{type_following, FileType, FilesError, Range};
use crate::environment::local::{make_local_driver_default, HostSpawner};
use crate::environment::memory::MemoryDriver;
use crate::environment::workspace::{WorkspaceError, WorkspaceService};
use crate::environment::{make_files, Driver, Environment};
use crate::location::LocationRef;
use crate::project::SqliteProjectStore;

async fn temp_root(tag: &str) -> PathBuf {
    let root = std::env::temp_dir()
        .join("oc-core-environment-tests")
        .join(format!("{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

async fn sqlite_store(tag: &str) -> (Arc<SqliteProjectStore>, PathBuf) {
    let root = temp_root(tag).await;
    let db = crate::database::Database::open_file(root.join("opencode.db")).await.unwrap();
    (Arc::new(SqliteProjectStore::new(db)), root)
}

// ===========================================================================
// Memory driver: files.rs semantics
// ===========================================================================

#[tokio::test]
async fn memory_stat_is_lstat_read_follows() {
    let driver = MemoryDriver::new();
    let files = make_files(&driver.driver());
    files.write("/a.txt", b"hello").await.unwrap();
    driver.symlink("/a.txt", "/link").await.unwrap();

    let stat = files.stat("/link").await.unwrap();
    assert_eq!(stat.kind, FileType::Symlink);
    assert_eq!(stat.size, "/a.txt".len() as u64);

    let read = files.read("/link", None).await.unwrap();
    assert_eq!(read.info.kind, FileType::File);
    assert_eq!(read.bytes, b"hello");

    // The original entry is untouched by the followed read.
    assert_eq!(files.stat("/link").await.unwrap().kind, FileType::Symlink);
}

#[tokio::test]
async fn memory_read_range_and_errors() {
    let driver = MemoryDriver::new();
    let files = make_files(&driver.driver());
    files.write("/a.txt", b"0123456789").await.unwrap();

    let read = files.read("/a.txt", Some(Range { offset: 2, length: 3 })).await.unwrap();
    assert_eq!(read.bytes, b"234");
    assert_eq!(read.info.size, 10);

    assert!(matches!(files.read("/missing", None).await, Err(FilesError::NotFound { .. })));
    assert!(matches!(
        files.read("/", None).await,
        Err(FilesError::WrongKind { actual: FileType::Directory, .. })
    ));
    // Dangling symlink: lookup sees the symlink, resolution fails NotFound.
    driver.symlink("/gone", "/dangling").await.unwrap();
    assert!(matches!(files.read("/dangling", None).await, Err(FilesError::NotFound { .. })));
}

#[tokio::test]
async fn memory_list_sorts_and_preserves_entry_types() {
    let driver = MemoryDriver::new();
    let files = make_files(&driver.driver());
    files.mkdir("/d").await.unwrap();
    files.write("/d/b.txt", b"").await.unwrap();
    files.write("/d/a.txt", b"").await.unwrap();
    files.mkdir("/d/sub").await.unwrap();
    driver.symlink("/a.txt", "/d/link").await.unwrap();

    let entries = files.list("/d").await.unwrap();
    let names: Vec<(&str, FileType)> =
        entries.iter().map(|entry| (entry.name.as_str(), entry.kind)).collect();
    assert_eq!(
        names,
        vec![
            ("a.txt", FileType::File),
            ("b.txt", FileType::File),
            ("link", FileType::Symlink),
            ("sub", FileType::Directory),
        ]
    );
}

// Wrong-kind list assertions live in `memory_list_wrong_kind`.

#[tokio::test]
async fn memory_list_wrong_kind() {
    let driver = MemoryDriver::new();
    let files = make_files(&driver.driver());
    files.write("/a.txt", b"x").await.unwrap();
    assert!(matches!(
        files.list("/a.txt").await,
        Err(FilesError::WrongKind { actual: FileType::File, .. })
    ));
    // List follows a final symlink to a directory.
    files.mkdir("/d").await.unwrap();
    driver.symlink("/d", "/dirlink").await.unwrap();
    let entries = files.list("/dirlink").await.unwrap();
    assert!(entries.is_empty());
}

#[tokio::test]
async fn memory_move_into_dir_and_not_found() {
    let driver = MemoryDriver::new();
    let files = make_files(&driver.driver());
    files.mkdir("/d").await.unwrap();
    files.write("/a.txt", b"x").await.unwrap();
    files.r#move("/a.txt", "/d").await.unwrap();
    assert!(matches!(files.stat("/a.txt").await, Err(FilesError::NotFound { .. })));
    assert_eq!(files.read("/d/a.txt", None).await.unwrap().bytes, b"x");

    assert!(matches!(files.r#move("/missing", "/d").await, Err(FilesError::NotFound { .. })));
}

#[tokio::test]
async fn memory_move_directory_tree() {
    let driver = MemoryDriver::new();
    let files = make_files(&driver.driver());
    files.mkdir("/src").await.unwrap();
    files.write("/src/inner.txt", b"x").await.unwrap();
    files.mkdir("/dst").await.unwrap();
    files.r#move("/src", "/dst").await.unwrap();
    assert_eq!(files.read("/dst/src/inner.txt", None).await.unwrap().bytes, b"x");
    // Moving a directory into itself fails.
    assert!(matches!(
        files.r#move("/dst/src", "/dst/src/sub/dir").await,
        Err(FilesError::Failed { .. })
    ));
}

#[tokio::test]
async fn memory_remove_is_recursive() {
    let driver = MemoryDriver::new();
    let files = make_files(&driver.driver());
    files.mkdir("/d/sub").await.unwrap();
    files.write("/d/sub/a.txt", b"x").await.unwrap();
    files.remove("/d").await.unwrap();
    assert!(matches!(files.stat("/d/sub/a.txt").await, Err(FilesError::NotFound { .. })));
}

#[tokio::test]
async fn memory_write_through_symlink_and_type_following() {
    let driver = MemoryDriver::new();
    let files = make_files(&driver.driver());
    files.write("/real.txt", b"first").await.unwrap();
    driver.symlink("/real.txt", "/link").await.unwrap();
    files.write("/link", b"second").await.unwrap();
    assert_eq!(files.read("/real.txt", None).await.unwrap().bytes, b"second");

    assert_eq!(type_following(files.as_ref(), "/link").await.unwrap(), FileType::File);
    files.mkdir("/d").await.unwrap();
    driver.symlink("/d", "/dirlink").await.unwrap();
    assert_eq!(type_following(files.as_ref(), "/dirlink").await.unwrap(), FileType::Directory);
    driver.symlink("/gone", "/dangling").await.unwrap();
    assert!(matches!(
        type_following(files.as_ref(), "/dangling").await,
        Err(FilesError::NotFound { .. })
    ));
}

// ===========================================================================
// exec_defaults (process-backed) over the host spawner
// ===========================================================================

#[tokio::test]
async fn exec_defaults_round_trip() {
    let root = temp_root("exec-roundtrip").await;
    let driver = Driver { spawner: Arc::new(HostSpawner), overrides: None, stdio: Arc::new(crate::environment::stdio::LocalStdioSpawner) };
    let files = make_files(&driver);
    let dir = root.join("d");
    let dir_str = dir.to_str().unwrap();

    files.mkdir(dir_str).await.unwrap();
    let file = dir.join("a.txt");
    files.write(file.to_str().unwrap(), b"0123456789").await.unwrap();

    let stat = files.stat(file.to_str().unwrap()).await.unwrap();
    assert_eq!(stat.kind, FileType::File);
    assert_eq!(stat.size, 10);
    assert!(stat.mtime_ms > 0.0);

    let read = files.read(file.to_str().unwrap(), None).await.unwrap();
    assert_eq!(read.bytes, b"0123456789");
    let ranged =
        files.read(file.to_str().unwrap(), Some(Range { offset: 2, length: 3 })).await.unwrap();
    assert_eq!(ranged.bytes, b"234");
}

#[tokio::test]
async fn exec_defaults_error_protocol() {
    let root = temp_root("exec-errors").await;
    let driver = Driver { spawner: Arc::new(HostSpawner), overrides: None, stdio: Arc::new(crate::environment::stdio::LocalStdioSpawner) };
    let files = make_files(&driver);

    // 44: NotFound.
    let missing = root.join("missing");
    assert!(matches!(
        files.stat(missing.to_str().unwrap()).await,
        Err(FilesError::NotFound { .. })
    ));
    // 45: WrongKind (read of a directory).
    assert!(matches!(
        files.read(root.to_str().unwrap(), None).await,
        Err(FilesError::WrongKind { actual: FileType::Directory, .. })
    ));
    // 46: Failed (move with a missing parent destination that stat can't see).
    // `mv` fails after loadMetadata succeeds on the source.
    let source = root.join("src.txt");
    files.write(source.to_str().unwrap(), b"x").await.unwrap();
    let blocked = root.join("blocked");
    std::fs::create_dir_all(&blocked).unwrap();
    std::fs::write(blocked.join(".gitkeep"), b"").unwrap();
    let mut perms = std::fs::metadata(&blocked).unwrap().permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(0o555);
    }
    std::fs::set_permissions(&blocked, perms).unwrap();
    let result = files.r#move(source.to_str().unwrap(), blocked.to_str().unwrap()).await;
    #[cfg(unix)]
    assert!(matches!(result, Err(FilesError::Failed { .. })));
    #[cfg(not(unix))]
    let _ = result;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&blocked).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&blocked, perms).unwrap();
    }
}

#[tokio::test]
async fn exec_defaults_symlink_semantics() {
    let root = temp_root("exec-symlink").await;
    let driver = Driver { spawner: Arc::new(HostSpawner), overrides: None, stdio: Arc::new(crate::environment::stdio::LocalStdioSpawner) };
    let files = make_files(&driver);
    let target = root.join("real.txt");
    std::fs::write(&target, b"body").unwrap();
    let link = root.join("link");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &link).unwrap();

    // stat is lstat: a final symlink reports `symlink`.
    let stat = files.stat(link.to_str().unwrap()).await.unwrap();
    assert_eq!(stat.kind, FileType::Symlink);
    // read follows the final symlink.
    let read = files.read(link.to_str().unwrap(), None).await.unwrap();
    assert_eq!(read.info.kind, FileType::File);
    assert_eq!(read.bytes, b"body");
    // list preserves each entry's own type.
    let entries = files.list(root.to_str().unwrap()).await.unwrap();
    let link_entry = entries.iter().find(|entry| entry.name == "link").unwrap();
    assert_eq!(link_entry.kind, FileType::Symlink);
}

#[tokio::test]
async fn exec_defaults_move_into_dir() {
    let root = temp_root("exec-move").await;
    let driver = Driver { spawner: Arc::new(HostSpawner), overrides: None, stdio: Arc::new(crate::environment::stdio::LocalStdioSpawner) };
    let files = make_files(&driver);
    files.mkdir(root.join("d").to_str().unwrap()).await.unwrap();
    let source = root.join("a.txt");
    files.write(source.to_str().unwrap(), b"x").await.unwrap();
    files.r#move(source.to_str().unwrap(), root.join("d").to_str().unwrap()).await.unwrap();
    assert!(std::fs::read(root.join("d/a.txt")).unwrap() == b"x");
    assert!(!source.exists());
}

// ===========================================================================
// Local driver (host fs overrides)
// ===========================================================================

#[tokio::test]
async fn local_driver_semantics() {
    let root = temp_root("local-driver").await;
    let files = make_files(&make_local_driver_default());
    let dir = root.join("d");
    files.mkdir(dir.to_str().unwrap()).await.unwrap();
    let file = dir.join("a.txt");
    files.write(file.to_str().unwrap(), b"hello").await.unwrap();
    assert_eq!(files.read(file.to_str().unwrap(), None).await.unwrap().bytes, b"hello");

    // Symlink: stat reports symlink, read follows.
    let link = root.join("link");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&file, &link).unwrap();
    #[cfg(unix)]
    {
        assert_eq!(files.stat(link.to_str().unwrap()).await.unwrap().kind, FileType::Symlink);
        assert_eq!(files.read(link.to_str().unwrap(), None).await.unwrap().bytes, b"hello");
    }

    // Move into a directory lands at basename join.
    files.r#move(file.to_str().unwrap(), dir.to_str().unwrap()).await.unwrap();
    assert!(std::fs::read(dir.join("a.txt")).unwrap() == b"hello");
    assert!(matches!(
        files.r#move(root.join("nope").to_str().unwrap(), dir.to_str().unwrap()).await,
        Err(FilesError::NotFound { .. })
    ));

    // Remove is recursive and force (absence is success).
    files.remove(dir.to_str().unwrap()).await.unwrap();
    files.remove(dir.to_str().unwrap()).await.unwrap();
    assert!(!dir.exists());
}

// ===========================================================================
// Workspace service round-trip
// ===========================================================================

#[tokio::test]
async fn workspace_create_provision_connect_destroy() {
    let (store, root) = sqlite_store("ws-roundtrip").await;
    let service = WorkspaceService::new(store.clone());

    let id = service.create(Some("wrk_a"), "local").await.unwrap();
    assert_eq!(id, "wrk_a");
    // Idempotent create with the same provider.
    assert_eq!(service.create(Some("wrk_a"), "local").await.unwrap(), "wrk_a");
    // Conflict on provider mismatch.
    assert!(matches!(
        service.create(Some("wrk_a"), "container").await,
        Err(WorkspaceError::CreateConflict { .. })
    ));

    // Provision persists a binding exactly once.
    let info = service.provision("wrk_a").await.unwrap();
    assert!(info.binding.is_some());
    let again = service.provision("wrk_a").await.unwrap();
    assert_eq!(again.binding, info.binding);

    // Connect returns a working driver and bumps last_used_at.
    let before = store.workspace_row("wrk_a").await.unwrap().4;
    let driver = service.connect("wrk_a").await.unwrap();
    let files = make_files(&driver);
    let probe = root.join("probe.txt");
    files.write(probe.to_str().unwrap(), b"ok").await.unwrap();
    assert_eq!(files.read(probe.to_str().unwrap(), None).await.unwrap().bytes, b"ok");
    let after = store.workspace_row("wrk_a").await.unwrap().4;
    assert!(after >= before);

    // Destroy removes the row and reports idempotently.
    assert!(service.destroy("wrk_a").await.unwrap().destroyed);
    assert!(!service.destroy("wrk_a").await.unwrap().destroyed);
    assert!(store.workspace_row("wrk_a").await.is_none());
    assert!(matches!(service.connect("wrk_a").await, Err(WorkspaceError::NotFound(_))));
}

#[tokio::test]
async fn workspace_unknown_provider() {
    let (store, _root) = sqlite_store("ws-unknown").await;
    let service = WorkspaceService::new(store);
    assert!(matches!(
        service.create(Some("wrk_b"), "container").await,
        Err(WorkspaceError::ProviderNotFound(provider)) if provider == "container"
    ));
    // Auto-generated id when omitted.
    let id = service.create(None, "local").await.unwrap();
    assert!(id.starts_with("wrk_"));
}

// ===========================================================================
// Environment per-location bind
// ===========================================================================

#[tokio::test]
async fn environment_bind_local_and_workspace() {
    let (store, root) = sqlite_store("env-bind").await;
    let environment = Environment::with_workspace(Arc::new(WorkspaceService::new(store.clone())));

    // No workspaceID: host driver.
    let local_files = environment.files(&LocationRef::new(root.clone(), None)).await.unwrap();
    let probe = root.join("local.txt");
    local_files.write(probe.to_str().unwrap(), b"local").await.unwrap();
    assert_eq!(local_files.read(probe.to_str().unwrap(), None).await.unwrap().bytes, b"local");

    // workspaceID: workspace driver (provisions on demand).
    store.insert_workspace("wrk_env", "local", None).await;
    let workspace_files = environment
        .files(&LocationRef::new(root.clone(), Some("wrk_env".to_string())))
        .await
        .unwrap();
    let probe = root.join("ws.txt");
    workspace_files.write(probe.to_str().unwrap(), b"ws").await.unwrap();
    assert_eq!(workspace_files.read(probe.to_str().unwrap(), None).await.unwrap().bytes, b"ws");
    assert!(store.workspace_row("wrk_env").await.unwrap().2.is_some());

    // Unknown workspace is a bind failure by design.
    let missing =
        environment.files(&LocationRef::new(root.clone(), Some("wrk_gone".to_string()))).await;
    assert!(missing.is_err());

    // Host-only environment rejects workspace locations.
    let host_only = Environment::local();
    assert!(host_only.bind(&LocationRef::new(root, Some("wrk_env".to_string()))).await.is_err());
}
