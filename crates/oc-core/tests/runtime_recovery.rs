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

use oc_core::database::schema::{ProjectRow, SessionRow};
use oc_core::bus::{Bus, BusOptions};
use oc_core::database::Database;
use oc_core::session::execution::{
    DrainFn, DrainOutcome, RunError, SessionExecution,
};
use oc_core::session::inbox::{Promotable, SessionInbox};
use oc_core::session::projector::install_projectors;
use oc_core::session::store::{MessagesInput, SessionStore};
use oc_core::session::subagent_job::SubagentJob;
use oc_core::subagent_jobs_adapter::JobsAdapter;
use oc_core::subagent::{ChildExecution, SubagentRecovery};
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
        self.execution.resume(session_id).await.map(|_| ()).map_err(|e| e.to_string())
    }

    fn wake(&self, session_id: &str) {
        self.execution.wake(session_id, Promotable::Input);
    }

    async fn interrupt(&self, session_id: &str) {
        use oc_core::session::execution::InterruptOptions;
        let _ = self
            .execution
            .interrupt(session_id, InterruptOptions { await_settlement: true, ..Default::default() })
            .await;
    }
}

async fn open_site_at(db_path: std::path::PathBuf, drain: DrainKind) -> Site {
    let db = Database::open_file(&db_path).await.expect("open db");
    let bus = Bus::new(
        db.clone(),
        BusOptions { persist: true, ..BusOptions::deterministic() },
    );
    install_projectors(&bus);
    let store = SessionStore::new(db.clone());
    let inbox = SessionInbox::new(db.clone(), bus.clone());
    let jobs = Arc::new(Jobs::new(
        db.clone(),
        Arc::new(SequentialIdGen::new()),
        Arc::new(SystemClock),
    ));
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
    let execution = Arc::new(SessionExecution::new(
        store.clone(),
        bus.clone(),
        inbox.clone(),
        None,
        drain_fn,
    ));
    let child: Arc<dyn ChildExecution> = Arc::new(SiteChildExecution { execution: Arc::clone(&execution) });
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
        ProjectRow::insert_or_ignore(tx, &ProjectRow::new("global", "/project", None).expect("project row"))?;
        let mut row = SessionRow::new(id, "global", "/project", "runtime recovery", "test")
            .expect("session row");
        row.parent_id = parent;
        SessionRow::insert_or_ignore(tx, &row)
    })
    .await
    .expect("seed session");
}

/// The site's message rows for a session.
#[allow(dead_code)] // used by the L01 cases below
async fn messages(site: &Site, session_id: &str) -> Vec<(String, String)> {
    site.store
        .messages(MessagesInput { session_id: session_id.to_string(), ..Default::default() })
        .await
        .expect("messages")
        .into_iter()
        .map(|row| (row.type_, row.data))
        .collect()
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
    assert!(site.jobs.pending_background().await.is_empty());

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
