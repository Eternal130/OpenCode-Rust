//! RUNTIME recovery regressions (ledger RUNTIME-S01, RUNTIME-L01):
//!
//! - **RUNTIME-S01** — a background subagent completion wakes the idle
//!   parent session: the production path is completion → `SubagentJob`
//!   observer → `subagent_completion::deliver` → inbox admit +
//!   `execution.wake(parent)` (v2 `sessions.synthetic`'s wake arm,
//!   session.ts:310). The parent consumes the result with NO further user
//!   message.
//! - **RUNTIME-L01** — the boot recovery sweep (`SessionRestart`
//!   port, v2 `session/execution/restart.ts`): orphaned claims are
//!   released/resumed, pending background records replay their
//!   notifications, and the durable `resume_attempts` budget
//!   terminalizes crash-looping turns.
//!
//! Hermetic: sandbox roots under `/tmp/opencode`, no network, no real
//! data dirs. Cross-process cases spawn ONLY this test binary's own
//! children (`kill_on_drop`, `Child::kill` — never name-based signals).

use std::sync::Arc;
use std::time::{Duration, Instant};

use oc_core::bus::{Bus, BusOptions};
use oc_core::database::schema::{ProjectRow, SessionRow};
use oc_core::database::Database;
use oc_core::session::execution::{DrainFn, DrainOutcome, RunError, SessionExecution};
use oc_core::session::inbox::{Promotable, SessionInbox};
use oc_core::session::projector::install_projectors;
use oc_core::session::store::{MessagesInput, SessionStore};
use oc_core::session::subagent_job::SubagentJob;
use oc_core::subagent::{ChildExecution, SubagentRecovery};
use oc_core::subagent_jobs_adapter::JobsAdapter;
use oc_util::determinism::{SequentialIdGen, SystemClock};

use oc_core::jobs::Jobs;

/// Poll until `probe` returns true (or the deadline lapses).
async fn eventually(timeout: Duration, mut probe: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if probe() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// One site's full runtime: DB + durable bus + production execution
/// coordinator (claim/release publication hooks) + the jobs registry +
/// the subagent-job machinery, all wired the way the engine assembles
/// them. Only the drain is injected — it promotes pending inbox input
/// exactly like the production runner boundary (runner/drain.rs `promote`)
/// and records which sessions drained.
struct Site {
    #[allow(dead_code)] // read by assertions through helpers
    db_path: std::path::PathBuf,
    db: Database,
    bus: Bus,
    store: SessionStore,
    inbox: SessionInbox,
    jobs: Arc<Jobs>,
    execution: Arc<SessionExecution>,
    subagents: Arc<SubagentJob>,
    /// Sessions that drained, in order (the wake proof).
    drained: Arc<std::sync::Mutex<Vec<String>>>,
}

enum DrainKind {
    /// Promote pending input, then complete (the production boundary).
    Consuming,
    /// Observe the shutdown watch like a wedged mid-turn drain: the
    /// resumed turn stays claimed until interrupted.
    Hanging,
}

/// The child-execution adapter over the site's own `SessionExecution`
/// (children share the parent's location in the engine; the test site has
/// exactly one execution service, so the same shape holds).
struct SiteChildExecution {
    execution: Arc<SessionExecution>,
}

#[async_trait::async_trait]
impl ChildExecution for SiteChildExecution {
    async fn resume(&self, session_id: &str) -> Result<(), String> {
        self.execution.resume(session_id).await.map_err(|e| e.to_string())
    }

    fn wake(&self, session_id: &str) {
        self.execution.wake(session_id, Promotable::Input);
    }

    async fn interrupt(&self, session_id: &str) {
        use oc_core::session::execution::InterruptOptions;
        let _ = self
            .execution
            .interrupt(
                session_id,
                InterruptOptions { await_settlement: true, ..Default::default() },
            )
            .await;
    }
}

async fn open_site_at(db_path: std::path::PathBuf, drain: DrainKind) -> Site {
    let db = Database::open_file(&db_path).await.expect("open db");
    let bus = Bus::new(db.clone(), BusOptions { persist: true, ..BusOptions::deterministic() });
    install_projectors(&bus);
    let store = SessionStore::new(db.clone());
    let inbox = SessionInbox::new(db.clone(), bus.clone());
    let jobs =
        Arc::new(Jobs::new(db.clone(), Arc::new(SequentialIdGen::new()), Arc::new(SystemClock)));
    let drained: Arc<std::sync::Mutex<Vec<String>>> = Arc::default();
    let drain_fn: DrainFn = {
        let drained = Arc::clone(&drained);
        let inbox = inbox.clone();
        match drain {
            DrainKind::Consuming => Arc::new(move |session, _force, _cont, scope, _shutdown| {
                let drained = Arc::clone(&drained);
                let inbox = inbox.clone();
                Box::pin(async move {
                    drained.lock().unwrap().push(session.clone());
                    inbox
                        .promote(&session, scope)
                        .await
                        .map_err(|error| RunError::Defect(format!("promote failed: {error}")))?;
                    Ok(DrainOutcome::Complete)
                })
            }),
            DrainKind::Hanging => Arc::new(move |session, _force, _cont, _scope, mut shutdown| {
                let drained = Arc::clone(&drained);
                Box::pin(async move {
                    drained.lock().unwrap().push(session.clone());
                    let _ = shutdown.changed().await;
                    Err(RunError::Interrupted)
                })
            }),
        }
    };
    let execution =
        Arc::new(SessionExecution::new(store.clone(), bus.clone(), inbox.clone(), None, drain_fn));
    let child: Arc<dyn ChildExecution> =
        Arc::new(SiteChildExecution { execution: Arc::clone(&execution) });
    let subagents = Arc::new(SubagentJob::new(
        store.clone(),
        inbox.clone(),
        Arc::clone(&child),
        Arc::new(JobsAdapter(Arc::clone(&jobs))),
    ));
    Site { db_path, db, bus, store, inbox, jobs, execution, subagents, drained }
}

async fn site(drain: DrainKind) -> Site {
    let dir = tempfile::Builder::new()
        .prefix("lane-runtime-a-")
        .tempdir_in("/tmp/opencode")
        .expect("sandbox under /tmp/opencode");
    let db_path = dir.path().join("opencode.db");
    // The sandbox dir stays alive for the test process's lifetime.
    std::mem::forget(dir);
    open_site_at(db_path, drain).await
}

/// Seeds a session row (plus its project row) like the projector would.
async fn seed_session(db: &Database, id: &str, parent: Option<&str>) {
    let id = id.to_string();
    let parent = parent.map(str::to_string);
    db.transaction(move |tx| {
        ProjectRow::insert_or_ignore(
            tx,
            &ProjectRow::new("global", "/project", None).expect("project row"),
        )?;
        let mut row = SessionRow::new(id, "global", "/project", "runtime recovery", "test")
            .expect("session row");
        row.parent_id = parent;
        SessionRow::insert_or_ignore(tx, &row)
    })
    .await
    .expect("seed session");
}

fn recovery(parent: &str, child: &str) -> SubagentRecovery {
    let sid = |raw: &str| oc_schema::id::SessionId::parse(raw.to_string()).expect("session id");
    SubagentRecovery {
        parent_session_id: sid(parent),
        child_session_id: sid(child),
        agent: "reviewer".to_string(),
        description: "background review".to_string(),
    }
}

fn drained_contains(site: &Site, session_id: &str) -> bool {
    site.drained.lock().unwrap().iter().any(|id| id == session_id)
}

// ---------------------------------------------------------------------------
// RUNTIME-S01 — background completion wakes the idle parent
// ---------------------------------------------------------------------------

/// The S01 exit criterion (PARITY-LOOP §4.4): the parent session is IDLE
/// (no user message, no active execution) when the backgrounded subagent
/// settles. The completion observer must admit the synthetic result AND
/// `execution.wake(parent)` it — the parent's drain promotes the result
/// into the transcript with no further user input.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn background_completion_wakes_idle_parent() {
    let site = site(DrainKind::Consuming).await;
    seed_session(&site.db, "ses_wake_parent", None).await;
    seed_session(&site.db, "ses_wake_child", Some("ses_wake_parent")).await;

    // The parent is idle: nothing has drained it and nothing will, except
    // the completion notification.
    assert!(!drained_contains(&site, "ses_wake_parent"));

    let rec = recovery("ses_wake_parent", "ses_wake_child");
    site.subagents.start(&rec).await;
    site.subagents.background(&rec).await;

    // The completion envelope lands in the parent's transcript without
    // any user message — only the wake could have started that drain.
    // (row.data is JSON-encoded: the envelope's quotes arrive escaped, so
    // match the marker prefix + ids instead of the raw quoted form.)
    let promoted = eventually(Duration::from_secs(15), || {
        messages_blocking(&site, "ses_wake_parent").iter().any(|(type_, data)| {
            type_ == "synthetic"
                && data.contains("<subagent sessionID=")
                && data.contains("ses_wake_child")
                && data.contains("background review")
        })
    })
    .await;
    assert!(
        promoted,
        "idle parent must consume the background result (envelope not in transcript)"
    );
    assert!(
        drained_contains(&site, "ses_wake_parent"),
        "the parent drained without a user message (execution.wake proof)"
    );
    // The durable notification marker is acknowledged.
    assert_eq!(site.jobs.pending_background().await.len(), 0);

    site.execution.await_idle("ses_wake_parent").await;
}

/// Blocking read for poll probes (the async store inside a sync closure).
fn messages_blocking(site: &Site, session_id: &str) -> Vec<(String, String)> {
    let store = site.store.clone();
    let id = session_id.to_string();
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async move {
            store.messages(MessagesInput { session_id: id, ..Default::default() }).await
        })
    })
    .expect("messages")
    .into_iter()
    .map(|row| (row.type_, row.data))
    .collect()
}

// ---------------------------------------------------------------------------
// RUNTIME-L01 — the boot recovery sweep (SessionRestart port)
// ---------------------------------------------------------------------------

use oc_core::jobs::{
    BackgroundRecord, Recovery as JobRecovery, RestartDeps, RestartOptions, Status as JobStatus,
    CONTINUE_AFTER_SERVER_RESTART, RESUME_EXHAUSTED_MESSAGE,
};
use oc_core::session::store::{attempts, claims};

/// The sweep over one site, forked through the site's execution handle
/// (v2 `Effect.forkIn(scope)`).
async fn sweep(site: &Site) {
    let execution = Arc::clone(&site.execution);
    let fork = move |id: String| {
        let execution = Arc::clone(&execution);
        tokio::spawn(async move {
            let _ = execution.resume(&id).await;
        });
    };
    let deps = RestartDeps {
        store: &site.store,
        inbox: &site.inbox,
        bus: &site.bus,
        jobs: &site.jobs,
        execution: &site.execution,
        subagents: &site.subagents,
        fork_resume: &fork,
    };
    deps.resume_suspended_sessions(&RestartOptions::default()).await;
}

/// Seed one durable background record (`job.background/<notificationID>`,
/// the exact serialization `Jobs::persist_background` writes).
async fn seed_background(site: &Site, record: &BackgroundRecord) {
    let key = format!("job.background/{}", record.notification_id);
    let value = serde_json::to_value(record).expect("record json");
    site.db.kv_set(&key, &value).await.expect("seed background");
}

async fn set_attempts(site: &Site, session_id: &str, value: i64) {
    let id = session_id.to_string();
    site.db
        .transaction(move |tx| {
            tx.execute(
                "UPDATE session_v2 SET resume_attempts = ?2 WHERE id = ?1",
                rusqlite::params![id, value],
            )?;
            Ok(())
        })
        .await
        .expect("set attempts");
}

fn subagent_record(
    parent: &str,
    child: &str,
    status: JobStatus,
    output: Option<&str>,
) -> BackgroundRecord {
    BackgroundRecord {
        id: format!("job_{child}"),
        notification_id: format!("msg_notif_{child}"),
        recovery: JobRecovery::Subagent {
            parent_session_id: parent.to_string(),
            child_session_id: child.to_string(),
            agent: "reviewer".to_string(),
            description: "background review".to_string(),
        },
        status,
        output: output.map(str::to_string),
        error: None,
    }
}

/// Boot-sweep claim hygiene (restart.ts:202-206): child claims orphaned by
/// the dead process are released — EXCEPT children owned by recoverable
/// background subagent jobs, whose claims survive so the recovery resume
/// keeps its durable budget trail. The recoverable child and the claimed
/// top-level parent are resumed (attempts counted); a wedge-style drain
/// keeps their claims so the assertions observe the in-flight state.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn boot_sweep_releases_orphan_claims() {
    let site = site(DrainKind::Hanging).await;
    seed_session(&site.db, "ses_sweep_parent", None).await;
    seed_session(&site.db, "ses_sweep_child", Some("ses_sweep_parent")).await;
    seed_session(&site.db, "ses_sweep_orphan", Some("ses_sweep_parent")).await;
    site.store.claim("ses_sweep_parent").await.expect("claim parent");
    site.store.claim("ses_sweep_child").await.expect("claim child");
    site.store.claim("ses_sweep_orphan").await.expect("claim orphan");
    let running = subagent_record("ses_sweep_parent", "ses_sweep_child", JobStatus::Running, None);
    seed_background(&site, &running).await;

    sweep(&site).await;

    // The orphan child's claim is released by release_child_claims.
    let map = claims(&site.db).await.expect("claims");
    let claimed = |id: &str| map.iter().find(|(row, _)| row == id).is_some_and(|(_, c)| *c);
    assert!(!claimed("ses_sweep_orphan"), "orphaned child claim must be released");
    // The recoverable child's claim survives (in-flight recovery resume).
    assert!(claimed("ses_sweep_child"), "recoverable child keeps its claim");

    // Parent and recoverable child both resume (durable attempt counts).
    assert!(
        eventually(Duration::from_secs(10), || {
            drained_contains(&site, "ses_sweep_parent")
                && drained_contains(&site, "ses_sweep_child")
        })
        .await,
        "the sweep resumed the suspended parent and the recoverable child"
    );
    assert_eq!(attempts(&site.db, "ses_sweep_parent").await.unwrap(), Some(1));
    assert_eq!(attempts(&site.db, "ses_sweep_child").await.unwrap(), Some(1));
    // The hanging drain keeps both claims (mid-turn state, no terminal yet).
    assert!(claimed("ses_sweep_parent"));
    assert!(claimed("ses_sweep_child"));
}

/// Boot-sweep pending-background replay (restart.ts:119-186): settled
/// records re-deliver their notification into the parent (waking it);
/// a record still marked running recovers as cancelled ("the server
/// restarted"). Shell outcomes admit before a recovered child can start
/// its first model request.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pending_background_replayed_after_restart() {
    let site = site(DrainKind::Consuming).await;
    seed_session(&site.db, "ses_replay_parent", None).await;
    seed_session(&site.db, "ses_replay_child", Some("ses_replay_parent")).await;
    let done = subagent_record(
        "ses_replay_parent",
        "ses_replay_child",
        JobStatus::Completed,
        Some("child finished text"),
    );
    seed_background(&site, &done).await;
    let shell = BackgroundRecord {
        id: "job_sh".to_string(),
        notification_id: "msg_notif_sh".to_string(),
        recovery: JobRecovery::Shell {
            session_id: "ses_replay_parent".to_string(),
            shell_id: "sh_1".to_string(),
            command: "sleep 1000".to_string(),
        },
        status: JobStatus::Running,
        output: None,
        error: None,
    };
    seed_background(&site, &shell).await;

    sweep(&site).await;

    // Both notifications reached the parent's transcript; the completed
    // subagent envelope carries its durable output.
    assert!(
        eventually(Duration::from_secs(10), || {
            let rows = messages_blocking(&site, "ses_replay_parent");
            rows.iter().any(|(type_, data)| {
                type_ == "synthetic"
                    && data.contains("<subagent sessionID=")
                    && data.contains("ses_replay_child")
                    && data.contains("child finished text")
            }) && rows.iter().any(|(type_, data)| {
                type_ == "synthetic"
                    && data.contains("<shell id=")
                    && data.contains("Command cancelled because the server restarted")
            })
        })
        .await,
        "pending background records must replay their notifications"
    );
    // The durable markers are acknowledged (notifications admitted).
    assert_eq!(site.jobs.pending_background().await.len(), 0);
}

/// The durable `resume_attempts` budget (restart.ts:67-84): every sweep
/// resume counts durably BEFORE resuming, and a turn that keeps dying is
/// terminalized (`session.execution.failed` + release) instead of resumed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn resume_attempts_increment_and_budget() {
    let site = site(DrainKind::Hanging).await;
    seed_session(&site.db, "ses_budget_once", None).await;
    seed_session(&site.db, "ses_budget_max", None).await;
    site.store.claim("ses_budget_once").await.expect("claim once");
    site.store.claim("ses_budget_max").await.expect("claim max");
    set_attempts(&site, "ses_budget_max", 10).await;

    sweep(&site).await;

    // One resume: the count incremented durably and the turn resumed.
    assert!(
        eventually(Duration::from_secs(10), || drained_contains(&site, "ses_budget_once")).await,
        "a fresh claim is resumed (attempt counted)"
    );
    assert_eq!(attempts(&site.db, "ses_budget_once").await.unwrap(), Some(1));

    // The exhausted turn is terminalized instead of resumed.
    let map = claims(&site.db).await.expect("claims");
    let max_claimed = map.iter().find(|(row, _)| row == "ses_budget_max").is_some_and(|(_, c)| *c);
    assert!(!max_claimed, "the exhausted turn's claim is released (terminalized)");
    assert_eq!(
        attempts(&site.db, "ses_budget_max").await.unwrap(),
        Some(0),
        "release resets the budget counter"
    );
    assert!(!drained_contains(&site, "ses_budget_max"), "the exhausted turn must NOT be resumed");
    // The terminal event is on the durable log with the v2 error shape.
    // (The stored `type` is versioned: "session.execution.failed.<v>".)
    let failed: Vec<(String, String)> = site
        .db
        .transaction(|tx| {
            let mut statement =
                tx.conn().prepare("SELECT type, data FROM event WHERE aggregate_id = ?1")?;
            let rows = statement
                .query_map(rusqlite::params!["ses_budget_max"], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .await
        .expect("events");
    assert!(
        failed.iter().any(|(type_, data)| {
            type_.starts_with("session.execution.failed")
                && data.contains("aborted")
                && data.contains(RESUME_EXHAUSTED_MESSAGE)
        }),
        "the exhausted turn terminalizes via session.execution.failed (aborted)"
    );
    // And the resumed session got the restart notice (prepareResume).
    assert!(
        eventually(Duration::from_secs(10), || {
            messages_blocking(&site, "ses_budget_once").iter().any(|(type_, data)| {
                type_ == "synthetic" && data.contains(CONTINUE_AFTER_SERVER_RESTART)
            })
        })
        .await,
        "prepareResume admits the continuing-after-restart notice"
    );
}

// ---------------------------------------------------------------------------
// RUNTIME-L01 — cross-process: kill -9 the "server" mid-turn, restart, sweep
// ---------------------------------------------------------------------------

const CHILD_ROLE_VAR: &str = "OC_RUNTIME_RECOVERY_ROLE";
const CHILD_DB_VAR: &str = "OC_RUNTIME_RECOVERY_DB";

fn spawn_role(role: &str, db_path: &std::path::Path) -> ChildGuard {
    let exe = std::env::current_exe().expect("current test exe");
    let child = std::process::Command::new(exe)
        .args(["--exact", "boot_sweep_recovers_orphans", "--nocapture"])
        .env(CHILD_ROLE_VAR, role)
        .env(CHILD_DB_VAR, db_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .expect("spawn child role");
    ChildGuard(child)
}

/// Process safety (PARITY-LOOP §2.1): the ONLY signal target is this
/// spawned child; the drop guard backstops early-return leaks (kill, then
/// reap — never a name-based signal).
struct ChildGuard(std::process::Child);

impl ChildGuard {
    fn kill(&mut self) {
        self.0.kill().expect("SIGKILL own child");
    }

    fn wait(&mut self) -> std::process::ExitStatus {
        self.0.wait().expect("reap child")
    }

    fn stdout(&mut self) -> Option<std::process::ChildStdout> {
        self.0.stdout.take()
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.0.try_wait().is_ok_and(|status| status.is_none()) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

/// Reads child stdout lines until one starts with `marker` (bounded).
/// `drain` keeps reading to EOF so the child's trailing harness output
/// never hits EPIPE (which would fail its exit status); the victim path
/// skips draining (the parent SIGKILLs it immediately).
fn read_until_marker(child: &mut ChildGuard, marker: &str, drain: bool) -> Option<String> {
    use std::io::BufRead;
    let deadline = Instant::now() + Duration::from_secs(60);
    let stdout = child.stdout().expect("piped stdout");
    let reader = std::io::BufReader::new(stdout);
    let mut found = None;
    for line in reader.lines() {
        let Ok(line) = line else { break };
        if found.is_none() && line.starts_with(marker) {
            found = Some(line);
            if !drain {
                break;
            }
        }
        if Instant::now() > deadline {
            break;
        }
    }
    found
}

/// The victim "server": seeds the durable signature of a process that
/// died mid-turn (claimed parent via the production claim primitive,
/// claimed orphan child, pending background records), announces READY,
/// then parks until the parent SIGKILLs it.
async fn victim_role(db_path: &std::path::Path) {
    let store_only = Database::open_file(db_path).await.expect("victim db");
    seed_session(&store_only, "ses_crash_parent", None).await;
    seed_session(&store_only, "ses_crash_child", Some("ses_crash_parent")).await;
    seed_session(&store_only, "ses_crash_orphan", Some("ses_crash_parent")).await;
    let store = SessionStore::new(store_only.clone());
    store.claim("ses_crash_parent").await.expect("claim parent");
    store.claim("ses_crash_child").await.expect("claim child");
    store.claim("ses_crash_orphan").await.expect("claim orphan");
    let done = subagent_record(
        "ses_crash_parent",
        "ses_crash_child",
        JobStatus::Completed,
        Some("cross-process result"),
    );
    store_only
        .kv_set(
            &format!("job.background/{}", done.notification_id),
            &serde_json::to_value(&done).expect("record"),
        )
        .await
        .expect("kv");
    println!("VICTIM_READY");
    // Park: the parent kills this process (SIGKILL) — no cleanup runs,
    // which is exactly the crash signature the sweep must recover.
    let deadline = Instant::now() + Duration::from_secs(120);
    while Instant::now() < deadline {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

/// The replacement "server": boots over the crashed database, runs the
/// boot recovery sweep, lets the resumed turn finish, reports JSON.
async fn replacement_role(db_path: &std::path::Path) {
    let site = open_site_at(db_path.to_path_buf(), DrainKind::Consuming).await;
    sweep(&site).await;
    // The resumed drain is forked: wait until the parent has actually
    // drained AND settled (a bare active()-empty poll would race the
    // fork's start and report too early).
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let active = site.execution.active();
        let parent_drained = drained_contains(&site, "ses_crash_parent");
        if parent_drained && active.is_empty() {
            break;
        }
        assert!(Instant::now() < deadline, "replacement: resumed parent never settled");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let map = claims(&site.db).await.expect("claims");
    let claimed = |id: &str| map.iter().find(|(row, _)| row == id).is_some_and(|(_, c)| *c);
    let rows = messages_blocking(&site, "ses_crash_parent");
    let envelope = rows.iter().any(|(type_, data)| {
        type_ == "synthetic"
            && data.contains("<subagent sessionID=")
            && data.contains("cross-process result")
    });
    let notice = rows
        .iter()
        .any(|(type_, data)| type_ == "synthetic" && data.contains(CONTINUE_AFTER_SERVER_RESTART));
    let report = serde_json::json!({
        "parent_claimed": claimed("ses_crash_parent"),
        "orphan_claimed": claimed("ses_crash_orphan"),
        "child_claimed": claimed("ses_crash_child"),
        "pending_bg": site.jobs.pending_background().await.len(),
        "envelope": envelope,
        "restart_notice": notice,
        "drained": site.drained.lock().unwrap().clone(),
    });
    println!("REPLACEMENT_RESULT {report}");
}

/// RUNTIME-L01 cross-process proof: SIGKILL a process holding a mid-turn
/// claim (own spawned child only), boot a replacement over the same
/// database, and assert the boot sweep recovers — orphan claim released,
/// suspended parent resumed (restart notice admitted), pending background
/// replayed into the parent's transcript, durable markers acknowledged.
#[test]
fn boot_sweep_recovers_orphans() {
    let role = std::env::var(CHILD_ROLE_VAR).ok();
    let db_path = std::env::var(CHILD_DB_VAR).map(std::path::PathBuf::from).ok();
    if let (Some(role), Some(db_path)) = (role, db_path) {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("child runtime");
        match role.as_str() {
            "victim" => runtime.block_on(victim_role(&db_path)),
            "replacement" => runtime.block_on(replacement_role(&db_path)),
            other => panic!("unknown role {other}"),
        }
        return;
    }

    let dir = tempfile::Builder::new()
        .prefix("lane-runtime-a-kill9-")
        .tempdir_in("/tmp/opencode")
        .expect("sandbox under /tmp/opencode");
    let db_path = dir.path().join("opencode.db");

    // 1. The victim server dies mid-turn: SIGKILL (own Child handle).
    let mut victim = spawn_role("victim", &db_path);
    assert!(
        read_until_marker(&mut victim, "VICTIM_READY", false).is_some(),
        "victim never reached its mid-turn state"
    );
    victim.kill();
    victim.wait();

    // 2. The replacement boots over the crashed database and sweeps.
    let mut replacement = spawn_role("replacement", &db_path);
    let line = read_until_marker(&mut replacement, "REPLACEMENT_RESULT ", true);
    let status = replacement.wait();
    assert!(status.success(), "replacement role exited {status}");
    let line = line.expect("replacement reported no result");
    let report: serde_json::Value =
        serde_json::from_str(line.strip_prefix("REPLACEMENT_RESULT ").expect("marker"))
            .expect("report json");

    // 3. Recovery assertions.
    assert_eq!(report["orphan_claimed"], false, "orphaned child claim released");
    assert_eq!(report["pending_bg"], 0, "durable background markers acknowledged");
    assert_eq!(report["envelope"], true, "pending background replayed to the parent");
    assert_eq!(report["restart_notice"], true, "resumed parent got the restart notice");
    assert_eq!(report["parent_claimed"], false, "resumed turn completed and released its claim");
    assert!(
        report["drained"]
            .as_array()
            .is_some_and(|ids| ids.iter().any(|id| id == "ses_crash_parent")),
        "the suspended parent actually drained after the restart"
    );
}
