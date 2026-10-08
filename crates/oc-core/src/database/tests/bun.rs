//! Acceptance tests against the pinned reference implementation
//! (opencode v2.0.24 at `/tmp/opencode/ref`, run with bun).
//!
//! Pattern copied from `tools/parity/src/reference.rs`:
//!
//! ```text
//! bun run --cwd /tmp/opencode/ref/packages/cli \
//!   --define OPENCODE_VERSION="2.0.24" \
//!   --define OPENCODE_CHANNEL="stable" \
//!   --define OPENCODE_ARTIFACT="cli" \
//!   src/index.ts serve --port <free>
//! ```
//!
//! - Auth is HTTP Basic (`opencode` / `OPENCODE_PASSWORD`).
//! - Readiness: `GET /api/info` answers 200.
//! - The child runs with a scrubbed environment inside a sandbox under
//!   `/tmp/opencode/core-db-sandbox/`; the live service state under
//!   `~/.local/state/opencode` is never reachable, and the managed-service
//!   port 49374 is never bound.
//!
//! Skipped when bun or the reference checkout is absent ([`has_bun`]).

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use base64::Engine as _;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};

use super::super::migrations::MIGRATIONS;
use super::super::Database;

/// Reference checkout root (override with `PARITY_REF_ROOT`).
const REF_ROOT: &str = "/tmp/opencode/ref";
/// Sandbox parent for these tests (override with `PARITY_SANDBOX_ROOT`).
const SANDBOX_ROOT: &str = "/tmp/opencode/core-db-sandbox";
/// The managed-service default port; never bound here.
const FORBIDDEN_PORT: u16 = 49374;
/// Deterministic password for the loopback test server.
const REF_PASSWORD: &str = "core-db-ref-password";

/// True when the reference can be run: bun exists (well-known install paths
/// or `PATH`) and the reference checkout is present.
pub(super) fn has_bun() -> bool {
    bun_path().is_some() && ref_root().is_dir()
}

fn ref_root() -> PathBuf {
    std::env::var("PARITY_REF_ROOT").map_or_else(|_| PathBuf::from(REF_ROOT), PathBuf::from)
}

fn bun_path() -> Option<PathBuf> {
    let well_known =
        ["/root/.bun/bin/bun", "/usr/local/bin/bun", "/usr/bin/bun", "/opt/homebrew/bin/bun"];
    if let Some(candidate) = well_known.iter().find(|c| Path::new(c).is_file()) {
        return Some(PathBuf::from(*candidate));
    }
    if let Ok(paths) = std::env::var("PATH") {
        for dir in paths.split(':') {
            let candidate = PathBuf::from(dir).join("bun");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn free_port() -> Option<u16> {
    for _ in 0..16 {
        let listener = TcpListener::bind("127.0.0.1:0").ok()?;
        let port = listener.local_addr().ok()?.port();
        drop(listener);
        if port != FORBIDDEN_PORT {
            return Some(port);
        }
    }
    None
}

struct ReferenceServer {
    child: Child,
    home: PathBuf,
}

impl ReferenceServer {
    /// Data dir the reference server's database lives in
    /// (`XDG_DATA_HOME/opencode`).
    fn data_dir(&self) -> PathBuf {
        self.home.join(".local/share/opencode")
    }

    async fn stop(mut self) {
        if let Some(pgid) = self.child.id() {
            // Signal the whole process group (`bun run` re-execs a child).
            let _ =
                std::process::Command::new("kill").args(["-TERM", &format!("-{pgid}")]).status();
        }
        if tokio::time::timeout(Duration::from_secs(5), self.child.wait())
            .await
            .is_ok_and(|status| status.is_ok())
        {
            return;
        }
        let _ = self.child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(5), self.child.wait()).await;
    }
}

fn prepare_sandbox(sandbox: &Path) -> std::io::Result<(PathBuf, PathBuf)> {
    let home = sandbox.join("home");
    let tmp = sandbox.join("tmp");
    for dir in [
        home.join(".local/share"),
        home.join(".local/state"),
        home.join(".config"),
        home.join(".cache"),
        tmp.clone(),
        sandbox.join("logs"),
    ] {
        std::fs::create_dir_all(dir)?;
    }
    Ok((home, tmp))
}

fn build_command(
    bun: &Path,
    root: &Path,
    port: u16,
    home: &Path,
    tmp: &Path,
    db: Option<&Path>,
) -> Command {
    let mut command = Command::new(bun);
    command
        .current_dir(root)
        .arg("run")
        .arg("--cwd")
        .arg(root.join("packages/cli"))
        .arg("--define")
        .arg("OPENCODE_VERSION=\"2.0.24\"")
        .arg("--define")
        .arg("OPENCODE_CHANNEL=\"stable\"")
        .arg("--define")
        .arg("OPENCODE_ARTIFACT=\"cli\"")
        .arg("src/index.ts")
        .arg("serve")
        .arg("--port")
        .arg(port.to_string())
        .env_clear()
        .env("PATH", "/root/.bun/bin:/usr/local/bin:/usr/bin:/bin")
        .env("HOME", home)
        .env("OPENCODE_TEST_HOME", home)
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("TMPDIR", tmp)
        .env("OPENCODE_PASSWORD", REF_PASSWORD)
        .env("OPENCODE_VERSION", "2.0.24")
        .env("OPENCODE_DISABLE_MODELS_FETCH", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(db) = db {
        command.env("OPENCODE_DB", db);
    }
    command.process_group(0);
    command
}

/// `GET /api/info` status code, or `None` on connection/timeout failure.
async fn info_status(port: u16) -> Option<u16> {
    let future = async {
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.ok()?;
        let auth =
            base64::engine::general_purpose::STANDARD.encode(format!("opencode:{REF_PASSWORD}"));
        let request = format!(
            "GET /api/info HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Basic {auth}\r\nAccept: */*\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(request.as_bytes()).await.ok()?;
        let mut head = [0u8; 128];
        let read = stream.read(&mut head).await.ok()?;
        let line = String::from_utf8_lossy(&head[..read]);
        let code = line.split_whitespace().nth(1)?;
        code.parse::<u16>().ok()
    };
    tokio::time::timeout(Duration::from_secs(2), future).await.ok()?
}

fn tee_to_log(pipe: impl tokio::io::AsyncRead + Send + Unpin + 'static, path: PathBuf) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(pipe).lines();
        let mut sink = std::fs::File::create(&path).ok();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(sink) = sink.as_mut() {
                use std::io::Write;
                let _ = writeln!(sink, "{line}");
            }
        }
    });
}

/// Spawns the reference server and waits for `GET /api/info` → 200.
async fn spawn_reference(db: Option<&Path>) -> anyhow_like::Result<ReferenceServer> {
    let bun = bun_path().ok_or("bun not found")?;
    let root = ref_root();
    let port = free_port().ok_or("no free port")?;
    let sandbox = std::env::var("PARITY_SANDBOX_ROOT")
        .map_or_else(|_| PathBuf::from(SANDBOX_ROOT), PathBuf::from)
        .join(format!("ref-{}-{port}", std::process::id()));
    let (home, tmp) = prepare_sandbox(&sandbox).map_err(|e| e.to_string())?;

    let mut child = build_command(&bun, &root, port, &home, &tmp, db)
        .spawn()
        .map_err(|e| format!("spawn {}: {e}", bun.display()))?;
    let logs = sandbox.join("logs");
    tee_to_log(child.stderr.take().expect("stderr piped"), logs.join("stderr.log"));
    tee_to_log(child.stdout.take().expect("stdout piped"), logs.join("stdout.log"));

    let deadline = Duration::from_secs(90);
    let started = Instant::now();
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!(
                "reference server exited during boot (status {status}); see {}",
                logs.display()
            )
            .into());
        }
        if started.elapsed() > deadline {
            return Err(format!(
                "reference server not ready after {}s; see {}",
                deadline.as_secs(),
                logs.display()
            )
            .into());
        }
        if info_status(port).await == Some(200) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Ok(ReferenceServer { child, home })
}

/// Minimal error alias (avoids a dev-dependency on anyhow).
mod anyhow_like {
    #[derive(Debug)]
    pub struct Error(pub String);
    impl std::fmt::Display for Error {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.0)
        }
    }
    impl From<&str> for Error {
        fn from(value: &str) -> Self {
            Self(value.to_string())
        }
    }
    impl From<String> for Error {
        fn from(value: String) -> Self {
            Self(value)
        }
    }
    pub type Result<T> = std::result::Result<T, Error>;
}

/// `sqlite_master` rows (`type, name, tbl_name, sql`) excluding sqlite
/// internals, ordered — the parity comparison payload.
fn sqlite_master_rows(path: &Path) -> Vec<(String, String, String, String)> {
    let conn = rusqlite::Connection::open(path).expect("open for sqlite_master compare");
    let mut statement = conn
        .prepare(
            "SELECT type, name, tbl_name, sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name",
        )
        .expect("prepare sqlite_master query");
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .expect("query sqlite_master")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("collect sqlite_master");
    rows
}

/// Waits for a `*.db` file under `dir` (up to `secs`).
async fn wait_for_db(dir: &Path, secs: u64) -> anyhow_like::Result<PathBuf> {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        let found: Vec<PathBuf> = std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .filter_map(std::result::Result::ok)
                    .map(|entry| entry.path())
                    .filter(|path| path.extension().is_some_and(|ext| ext == "db"))
                    .collect()
            })
            .unwrap_or_default();
        match found.len() {
            0 => {}
            1 => return Ok(found.into_iter().next().expect("single entry")),
            n => return Err(format!("expected one .db under {}, found {n}", dir.display()).into()),
        }
        if Instant::now() > deadline {
            return Err(format!("no .db appeared under {} after {secs}s", dir.display()).into());
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Acceptance (a): open a real v2.0.24-created database; assert the 48-id
/// journal, zero re-runs, and a write. Also acceptance (c): compare
/// `sqlite_master` (tables + indexes, including DDL text) against a
/// Rust-bootstrapped database.
#[tokio::test]
async fn database_reference_db_opens_and_journal_parities() {
    if !has_bun() {
        eprintln!("skipping: bun or reference checkout absent");
        return;
    }
    let server = spawn_reference(None).await.expect("spawn reference server");

    // The reference builds `stable`-channel databases.
    let db_file = wait_for_db(&server.data_dir(), 15).await.expect("reference db file");
    assert_eq!(
        db_file.file_name().and_then(|n| n.to_str()),
        Some("opencode-stable.db"),
        "channel scheme filename"
    );
    server.stop().await;

    // --- Acceptance (a): read side ---
    let db = Database::open_file(&db_file).await.expect("open reference-created db");
    let report = db.apply_report().expect("report");
    assert!(!report.bootstrapped, "reference DB must not re-bootstrap");
    assert!(
        report.applied.is_empty(),
        "v2.0.24 DB needs zero migrations: applied {:?}",
        report.applied
    );

    let ids = db.migration_ids().await.expect("journal ids");
    let expected = MIGRATIONS.iter().map(|m| m.id.to_string()).collect::<Vec<_>>();
    assert_eq!(ids, expected, "journal ids match the 48 in order");

    db.kv_set("rust-core-db-lane", &serde_json::json!({"from": "rust"}))
        .await
        .expect("write kv row into reference db");
    assert_eq!(
        db.kv_get("rust-core-db-lane").await.unwrap().unwrap(),
        serde_json::json!({"from": "rust"})
    );
    drop(db);

    // --- Acceptance (c): sqlite_master parity ---
    let sandbox = std::env::var("PARITY_SANDBOX_ROOT")
        .map_or_else(|_| PathBuf::from(SANDBOX_ROOT), PathBuf::from);
    let rust_dir = sandbox.join(format!("rust-{}", std::process::id()));
    std::fs::create_dir_all(&rust_dir).expect("create rust sandbox dir");
    let rust_db_path = rust_dir.join("opencode.db");
    let rust_db = Database::open_file(&rust_db_path).await.expect("bootstrap rust db");
    assert!(rust_db.apply_report().expect("report").bootstrapped);
    drop(rust_db);

    let reference_master = sqlite_master_rows(&db_file);
    let rust_master = sqlite_master_rows(&rust_db_path);
    assert_eq!(
        reference_master, rust_master,
        "bootstrap DDL must produce the same sqlite_master as the reference"
    );
    assert_eq!(reference_master.len(), 36, "19 tables + migration + 16 indexes");
}

/// Acceptance (b): a Rust-bootstrapped database boots the reference server
/// cleanly (proves write-side journal/DDL interop).
#[tokio::test]
async fn database_reference_boots_on_rust_bootstrapped_db() {
    if !has_bun() {
        eprintln!("skipping: bun or reference checkout absent");
        return;
    }
    let sandbox = std::env::var("PARITY_SANDBOX_ROOT")
        .map_or_else(|_| PathBuf::from(SANDBOX_ROOT), PathBuf::from);
    let dir = sandbox.join(format!("write-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create write sandbox");
    let db_path = dir.join("opencode.db");

    {
        let db = Database::open_file(&db_path).await.expect("rust bootstrap");
        let report = db.apply_report().expect("report");
        assert!(report.bootstrapped);
        assert_eq!(db.migration_ids().await.unwrap().len(), 48);
    }

    let mut server = spawn_reference(Some(&db_path)).await.expect("reference boots on rust db");

    // "Boots clean": readiness 200 (asserted by spawn_reference) and the
    // process is still alive; then stop and confirm the journal is untouched.
    assert!(server.child.try_wait().expect("child alive").is_none());
    server.stop().await;

    let db = Database::open_file(&db_path).await.expect("reopen after reference boot");
    let report = db.apply_report().expect("report");
    assert!(
        report.applied.is_empty(),
        "reference must not have re-run migrations: {:?}",
        report.applied
    );
    assert!(!report.bootstrapped);
    assert_eq!(db.migration_ids().await.unwrap().len(), 48);
}
