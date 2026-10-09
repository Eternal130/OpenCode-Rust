//! Port of `packages/core/src/job.ts` — scoped, process-local job registry
//! with durable background records (§6.2).
//!
//! Semantics (job.ts:171-177): explicitly recoverable background work also
//! owns a durable notification marker (`job.background/<notificationID>` in
//! KV) until its notification is admitted. Unconsumed results survive the
//! start-to-wait handoff. Foreground block/cancel and non-recoverable wait
//! results enter a 25-entry consumed history. Recoverable wait results stay
//! available for background registration and acknowledgment.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use oc_util::determinism::{Clock, IdGen};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

const BACKGROUND_PREFIX: &str = "job.background/";
const COMPLETED_LIMIT: usize = 25;

/// `Job.Recovery` — how to re-notify a background job after restart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Recovery {
    Shell {
        session_id: String,
        shell_id: String,
        command: String,
    },
    Subagent {
        parent_session_id: String,
        child_session_id: String,
        agent: String,
        description: String,
    },
}

/// `Job.Status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Running,
    Completed,
    Error,
    Cancelled,
}

/// `Job.Info`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Info {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub status: Status,
    pub started_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notification_id: Option<String>,
}

/// The run body's error: a rendered message plus whether the cause was an
/// interrupt (interrupts-only ⇒ `cancelled`).
#[derive(Debug, Clone)]
pub struct RunError {
    pub message: String,
    pub interrupted: bool,
}

impl RunError {
    pub fn message(message: impl Into<String>) -> Self {
        Self { message: message.into(), interrupted: false }
    }
}

/// `Job.StartInput.run` — settles with the output text or fails.
pub type Run = futures::future::BoxFuture<'static, Result<String, RunError>>;

/// `Job.StartInput`.
pub struct StartInput {
    pub id: String,
    #[allow(clippy::type_complexity)]
    pub type_: String,
    pub title: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub recovery: Option<Recovery>,
    pub notification_id: Option<String>,
    pub run: Run,
}

/// `Job.WaitResult`.
#[derive(Debug, Clone, PartialEq)]
pub struct WaitResult {
    pub info: Option<Info>,
    pub timed_out: bool,
}

/// `Job.BlockResult`.
#[derive(Debug, Clone, PartialEq)]
pub enum BlockResult {
    Finished { info: Info },
    Backgrounded { info: Info },
}

/// `Job.BackgroundAllInput`.
pub struct BackgroundAllInput<'a> {
    pub session_id: &'a str,
    pub type_: Option<&'a str>,
}

/// The durable background record (`job.background/<id>` value).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BackgroundRecord {
    pub id: String,
    pub notification_id: String,
    pub recovery: Recovery,
    pub status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

struct ActiveJob {
    info: Mutex<Info>,
    /// Resolves with the terminal `Info` (multi-waiter broadcast).
    done: watch::Sender<Option<Info>>,
    /// Resolves when the job is backgrounded while running.
    backgrounded: watch::Sender<Option<Info>>,
    handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
    blocking_sessions: Mutex<HashMap<String, usize>>,
    is_backgrounded: Mutex<bool>,
    consumed: Mutex<bool>,
    recovery: Option<Recovery>,
}

impl ActiveJob {
    fn snapshot(&self) -> Info {
        self.info.lock().clone()
    }

    fn terminal(&self) -> Option<Info> {
        self.done.borrow().clone()
    }
}

/// The job registry (clone shares state).
#[derive(Clone)]
pub struct Jobs {
    inner: Arc<Inner>,
}

struct Inner {
    db: crate::database::Database,
    ids: Arc<dyn IdGen>,
    clock: Arc<dyn Clock>,
    jobs: Mutex<HashMap<String, Arc<ActiveJob>>>,
}

impl Jobs {
    pub fn new(db: crate::database::Database, ids: Arc<dyn IdGen>, clock: Arc<dyn Clock>) -> Self {
        Self { inner: Arc::new(Inner { db, ids, clock, jobs: Mutex::new(HashMap::new()) }) }
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.inner.clock.unix_ms()).unwrap_or(0)
    }

    fn kv_key(notification_id: &str) -> String {
        format!("{BACKGROUND_PREFIX}{notification_id}")
    }

    async fn persist_background(&self, job: &ActiveJob) {
        let Some(recovery) = &job.recovery else { return };
        let (id, notification_id, status, output, error) = {
            let info = job.info.lock();
            let Some(notification_id) = &info.notification_id else { return };
            (
                info.id.clone(),
                notification_id.clone(),
                info.status,
                info.output.clone(),
                info.error.clone(),
            )
        };
        let record = BackgroundRecord {
            id,
            notification_id: notification_id.clone(),
            recovery: recovery.clone(),
            status,
            output,
            error,
        };
        let key = Self::kv_key(&notification_id);
        let value = serde_json::to_value(&record).unwrap_or(serde_json::Value::Null);
        let _ = self.inner.db.kv_set(&key, &value).await;
    }

    /// `Job.get`.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<Info> {
        self.inner.jobs.lock().get(id).map(|job| job.snapshot())
    }

    /// Consumption history ordering: delete + reinsert keeps first-consumption
    /// order, then trims to [`COMPLETED_LIMIT`] consumed non-notifying jobs.
    fn consume(&self, id: &str, job: &Arc<ActiveJob>) {
        let mut jobs = self.inner.jobs.lock();
        let Some(current) = jobs.get(id) else { return };
        if !Arc::ptr_eq(current, job) {
            return;
        }
        if current.info.lock().status == Status::Running || *current.consumed.lock() {
            return;
        }
        *current.consumed.lock() = true;
        let value = jobs.remove(id).unwrap_or_else(|| Arc::clone(job));
        jobs.insert(id.to_string(), value);
        let consumed: Vec<String> = jobs
            .iter()
            .filter(|(_, job)| *job.consumed.lock() && job.info.lock().notification_id.is_none())
            .map(|(id, _)| id.clone())
            .collect();
        if consumed.len() > COMPLETED_LIMIT {
            for id in &consumed[..consumed.len() - COMPLETED_LIMIT] {
                jobs.remove(id);
            }
        }
    }

    /// Settles a finished run. Verifies the map entry is still the same job
    /// (generation identity) before mutating.
    async fn settle(
        &self,
        id: &str,
        job: &Arc<ActiveJob>,
        result: Result<String, RunError>,
    ) -> Option<Info> {
        let same = self.inner.jobs.lock().get(id).is_some_and(|current| Arc::ptr_eq(current, job));
        if !same {
            return None;
        }
        let already = job.terminal().is_some();
        if already {
            return job.terminal();
        }
        let completed_at = self.now_ms();
        {
            let mut info = job.info.lock();
            match &result {
                Ok(output) => {
                    info.status = Status::Completed;
                    info.output = Some(output.clone());
                }
                Err(error) => {
                    info.status = if error.interrupted { Status::Cancelled } else { Status::Error };
                    info.error = Some(error.message.clone());
                }
            }
            info.completed_at = Some(completed_at);
        }
        let snapshot = job.snapshot();
        if snapshot.status != Status::Cancelled {
            self.persist_background(job).await;
        }
        let _ = job.done.send(Some(snapshot.clone()));
        snapshot.into()
    }

    /// `Job.start` — registers the job, then forks the run.
    #[must_use]
    pub fn start(&self, input: StartInput) -> Info {
        let mut jobs = self.inner.jobs.lock();
        if let Some(existing) = jobs.get(&input.id) {
            if existing.info.lock().status == Status::Running {
                return existing.snapshot();
            }
        }
        let started_at = self.now_ms();
        let info = Info {
            id: input.id.clone(),
            type_: input.type_,
            title: input.title,
            status: Status::Running,
            started_at,
            metadata: input.metadata,
            notification_id: input.notification_id,
            output: None,
            error: None,
            completed_at: None,
        };
        let (done_tx, _) = watch::channel(None);
        let (backgrounded_tx, _) = watch::channel(None);
        let job = Arc::new(ActiveJob {
            info: Mutex::new(info.clone()),
            done: done_tx,
            backgrounded: backgrounded_tx,
            handle: Mutex::new(None),
            blocking_sessions: Mutex::new(HashMap::new()),
            is_backgrounded: Mutex::new(false),
            consumed: Mutex::new(false),
            recovery: input.recovery,
        });
        jobs.insert(input.id.clone(), Arc::clone(&job));
        drop(jobs);

        let jobs = self.clone();
        let id = input.id.clone();
        let job_for_task = Arc::clone(&job);
        let handle = tokio::spawn(async move {
            let result = input.run.await;
            jobs.settle(&id, &job_for_task, result).await;
        });
        *job.handle.lock() = Some(handle);
        info
    }

    /// `Job.wait`.
    pub async fn wait(&self, id: &str, timeout: Option<u64>) -> WaitResult {
        let Some(job) = self.inner.jobs.lock().get(id).cloned() else {
            return WaitResult { info: None, timed_out: false };
        };
        let result = if job.info.lock().status == Status::Running {
            match timeout {
                None => {
                    let mut receiver = job.done.subscribe();
                    loop {
                        {
                            let current = receiver.borrow();
                            if let Some(info) = current.as_ref() {
                                break WaitResult { info: Some(info.clone()), timed_out: false };
                            }
                        }
                        if receiver.changed().await.is_err() {
                            break WaitResult { info: Some(job.snapshot()), timed_out: false };
                        }
                    }
                }
                Some(0) => WaitResult { info: Some(job.snapshot()), timed_out: true },
                Some(timeout) => {
                    let mut receiver = job.done.subscribe();
                    loop {
                        {
                            let current = receiver.borrow();
                            if let Some(info) = current.as_ref() {
                                break WaitResult { info: Some(info.clone()), timed_out: false };
                            }
                        }
                        match tokio::time::timeout(
                            Duration::from_millis(timeout),
                            receiver.changed(),
                        )
                        .await
                        {
                            Ok(Ok(())) => {}
                            Ok(Err(_)) => {
                                break WaitResult { info: Some(job.snapshot()), timed_out: false }
                            }
                            Err(_) => {
                                break WaitResult { info: Some(job.snapshot()), timed_out: true }
                            }
                        }
                    }
                }
            }
        } else {
            WaitResult { info: Some(job.snapshot()), timed_out: false }
        };
        let running = result.info.as_ref().is_some_and(|info| info.status == Status::Running);
        if !running && job.recovery.is_none() {
            self.consume(id, &job);
        }
        result
    }

    fn remove_block(&self, id: &str, session_id: &str) {
        let jobs = self.inner.jobs.lock();
        let Some(job) = jobs.get(id) else { return };
        if job.info.lock().status != Status::Running || *job.is_backgrounded.lock() {
            return;
        }
        let mut sessions = job.blocking_sessions.lock();
        match sessions.get(session_id).copied() {
            None => {}
            Some(1) => {
                sessions.remove(session_id);
            }
            Some(count) => {
                sessions.insert(session_id.to_string(), count - 1);
            }
        }
    }

    /// `Job.block` — wait foreground, racing completion against backgrounding.
    pub async fn block(&self, id: &str, session_id: &str) -> Option<BlockResult> {
        enum Outcome {
            Done(Info),
            Backgrounded(Info),
        }
        let (job, initial) = {
            let jobs = self.inner.jobs.lock();
            let job = jobs.get(id).cloned()?;
            let initial = match job.terminal() {
                Some(info) => Some(BlockResult::Finished { info }),
                None => {
                    if *job.is_backgrounded.lock() {
                        Some(BlockResult::Backgrounded { info: job.snapshot() })
                    } else {
                        None
                    }
                }
            };
            (job, initial)
        };
        if let Some(initial) = initial {
            if let BlockResult::Finished { .. } = &initial {
                self.consume(id, &job);
            }
            return Some(initial);
        }
        {
            let mut sessions = job.blocking_sessions.lock();
            *sessions.entry(session_id.to_string()).or_insert(0) += 1;
        }
        let outcome = {
            let mut done = job.done.subscribe();
            let mut backgrounded = job.backgrounded.subscribe();
            loop {
                if let Some(info) = done.borrow().as_ref() {
                    break Outcome::Done(info.clone());
                }
                if let Some(info) = backgrounded.borrow().as_ref() {
                    break Outcome::Backgrounded(info.clone());
                }
                tokio::select! {
                    changed = done.changed() => {
                        if changed.is_err() {
                            break Outcome::Done(job.snapshot());
                        }
                    }
                    _changed = backgrounded.changed() => {}
                }
            }
        };
        let result = match outcome {
            Outcome::Done(info) => {
                self.consume(id, &job);
                BlockResult::Finished { info }
            }
            Outcome::Backgrounded(info) => BlockResult::Backgrounded { info },
        };
        self.remove_block(id, session_id);
        Some(result)
    }

    /// `markBackground` — background in place, allocating a notification id
    /// for recoverable work and persisting the durable record.
    async fn mark_background(&self, job: &Arc<ActiveJob>) -> Info {
        *job.is_backgrounded.lock() = true;
        job.blocking_sessions.lock().clear();
        if job.recovery.is_some() {
            let mut info = job.info.lock();
            if info.notification_id.is_none() {
                info.notification_id = Some(format!("msg_{}", self.inner.ids.new_id("msg")));
            }
        }
        let snapshot = job.snapshot();
        self.persist_background(job).await;
        snapshot
    }

    /// `Job.background`.
    pub async fn background(&self, id: &str) -> Option<Info> {
        let job = {
            let jobs = self.inner.jobs.lock();
            let job = jobs.get(id).cloned()?;
            // Recoverable work may finish before the caller backgrounds it.
            if job.info.lock().status != Status::Running && job.recovery.is_none() {
                return None;
            }
            if *job.is_backgrounded.lock() {
                return Some(job.snapshot());
            }
            job
        };
        let snapshot = self.mark_background(&job).await;
        let _ = job.backgrounded.send(Some(snapshot.clone()));
        Some(snapshot)
    }

    /// `Job.backgroundAll`.
    pub async fn background_all(&self, input: BackgroundAllInput<'_>) -> Vec<Info> {
        let mut marked = Vec::new();
        {
            let jobs = self.inner.jobs.lock();
            for (id, job) in jobs.iter() {
                if job.info.lock().status != Status::Running {
                    continue;
                }
                if *job.is_backgrounded.lock() {
                    continue;
                }
                if let Some(type_) = input.type_ {
                    if job.info.lock().type_ != type_ {
                        continue;
                    }
                }
                if !job.blocking_sessions.lock().contains_key(input.session_id) {
                    continue;
                }
                marked.push((id.clone(), Arc::clone(job)));
            }
        }
        let mut infos = Vec::new();
        for (_, job) in &marked {
            infos.push(self.mark_background(job).await);
            let _ = job.backgrounded.send(Some(job.snapshot()));
        }
        infos
    }

    /// `Job.cancel`.
    pub async fn cancel(&self, id: &str) -> Option<Info> {
        let job = self.inner.jobs.lock().get(id).cloned()?;
        if job.info.lock().status != Status::Running {
            let info = job.snapshot();
            self.consume(id, &job);
            return Some(info);
        }
        let completed_at = self.now_ms();
        {
            let mut info = job.info.lock();
            info.status = Status::Cancelled;
            info.completed_at = Some(completed_at);
            job.blocking_sessions.lock().clear();
        }
        let snapshot = job.snapshot();
        self.persist_background(&job).await;
        // Resolve waiters before aborting the run so a same-ID replacement
        // can start immediately.
        let _ = job.done.send(Some(snapshot.clone()));
        if let Some(handle) = job.handle.lock().take() {
            handle.abort();
        }
        self.consume(id, &job);
        Some(snapshot)
    }

    /// `Job.pendingBackground` — recovery records until their notification is
    /// admitted (restart re-notify).
    pub async fn pending_background(&self) -> Vec<BackgroundRecord> {
        let keys: Vec<(String, String)> = self
            .inner
            .db
            .with_conn(move |conn| {
                let mut statement =
                    conn.prepare("SELECT key, value FROM kv WHERE key LIKE ?1 ORDER BY key")?;
                let rows = statement
                    .query_map(rusqlite::params![format!("{BACKGROUND_PREFIX}%")], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await
            .unwrap_or_default();
        keys.into_iter().filter_map(|(_, value)| serde_json::from_str(&value).ok()).collect()
    }

    /// `Job.completeBackground` — the notification was admitted.
    pub async fn complete_background(&self, notification_id: &str) {
        let key = Self::kv_key(notification_id);
        let _ = self.inner.db.transaction(move |tx| tx.kv_delete(&key)).await;
        let jobs = self.inner.jobs.lock();
        let entry = jobs
            .iter()
            .find(|(_, job)| job.info.lock().notification_id.as_deref() == Some(notification_id));
        if let Some((id, job)) = entry {
            if job.info.lock().status != Status::Running {
                let id = id.clone();
                drop(jobs);
                self.inner.jobs.lock().remove(&id);
            }
        }
    }

    /// Cancels every running background job of `session_id` — the
    /// `CancelJobs` hook wired into `SessionExecution.interrupt` (v2 cancels
    /// a session's jobs on non-shutdown interrupts; here it covers all
    /// running jobs whose metadata carries the session).
    pub async fn cancel_session(&self, session_id: &str) {
        let targets: Vec<String> = {
            let jobs = self.inner.jobs.lock();
            jobs.iter()
                .filter(|(_, job)| {
                    let info = job.info.lock();
                    info.status == Status::Running
                        && info
                            .metadata
                            .as_ref()
                            .and_then(|metadata| metadata.get("sessionID"))
                            .and_then(|value| value.as_str())
                            .is_some_and(|value| value == session_id)
                })
                .map(|(id, _)| id.clone())
                .collect()
        };
        for id in targets {
            self.cancel(&id).await;
        }
    }
}

#[async_trait::async_trait]
impl crate::session::execution::CancelJobs for Jobs {
    async fn cancel(&self, session_id: &str) {
        self.cancel_session(session_id).await;
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::assert_is_empty)] // deliberate empty-state assertions

    use super::*;
    use oc_util::determinism::{SequentialIdGen, SystemClock};

    async fn jobs() -> Jobs {
        let db = crate::database::Database::open_in_memory().await.unwrap();
        Jobs::new(db, Arc::new(SequentialIdGen::new()), Arc::new(SystemClock))
    }

    fn latch_run() -> (Run, Arc<tokio::sync::Notify>) {
        let notify = Arc::new(tokio::sync::Notify::new());
        let waiter = Arc::clone(&notify);
        let run: Run = Box::pin(async move {
            waiter.notified().await;
            Ok("done".to_string())
        });
        (run, notify)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn job_tracks_process_local_work_through_explicit_observation() {
        let jobs = jobs().await;
        let (run, latch) = latch_run();
        let job = jobs.start(StartInput {
            id: "job_metadata".into(),
            type_: "test".into(),
            title: None,
            metadata: Some(serde_json::json!({"durable": false})),
            recovery: None,
            notification_id: None,
            run,
        });
        assert_eq!(job.type_, "test");
        assert_eq!(job.status, Status::Running);
        assert_eq!(job.metadata, Some(serde_json::json!({"durable": false})));

        let waiting = jobs.wait("job_metadata", Some(0)).await;
        assert!(waiting.timed_out);
        assert_eq!(waiting.info.unwrap().status, Status::Running);

        latch.notify_one();
        let result = jobs.wait("job_metadata", None).await;
        assert!(!result.timed_out);
        let info = result.info.unwrap();
        assert_eq!(info.status, Status::Completed);
        assert_eq!(info.output.as_deref(), Some("done"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn job_reuses_running_work_when_started_again_with_the_same_id() {
        let jobs = jobs().await;
        let (run, latch) = latch_run();
        let job = jobs.start(StartInput {
            id: "job_reused".into(),
            type_: "test".into(),
            title: None,
            metadata: None,
            recovery: None,
            notification_id: None,
            run,
        });
        let duplicate = jobs.start(StartInput {
            id: "job_reused".into(),
            type_: "duplicate".into(),
            title: None,
            metadata: None,
            recovery: None,
            notification_id: None,
            run: Box::pin(async { Err(RunError::message("Duplicate work must not run")) }),
        });
        assert_eq!(duplicate, job);
        latch.notify_one();
        let result = jobs.wait("job_reused", None).await;
        let info = result.info.unwrap();
        assert_eq!(info.type_, "test");
        assert_eq!(info.status, Status::Completed);
        assert_eq!(info.output.as_deref(), Some("done"));
    }

    #[tokio::test]
    async fn job_returns_finished_from_a_blocking_wait_when_completion_wins() {
        let jobs = jobs().await;
        let (run, latch) = latch_run();
        let _ = jobs.start(StartInput {
            id: "job_finished".into(),
            type_: "test".into(),
            title: None,
            metadata: None,
            recovery: None,
            notification_id: None,
            run,
        });
        let waiter = {
            let jobs = jobs.clone();
            tokio::spawn(async move { jobs.block("job_finished", "ses_parent").await })
        };
        tokio::task::yield_now().await;
        latch.notify_one();
        let result = waiter.await.unwrap().unwrap();
        assert!(matches!(result, BlockResult::Finished { .. }));
        assert_eq!(jobs.background("job_finished").await, None);
    }

    #[tokio::test]
    async fn job_returns_backgrounded_from_a_blocking_wait_when_background_wins() {
        let jobs = jobs().await;
        let (run, latch) = latch_run();
        let _ = jobs.start(StartInput {
            id: "job_backgrounded".into(),
            type_: "test".into(),
            title: None,
            metadata: None,
            recovery: None,
            notification_id: None,
            run,
        });
        let waiter = {
            let jobs = jobs.clone();
            tokio::spawn(async move { jobs.block("job_backgrounded", "ses_parent").await })
        };
        tokio::task::yield_now().await;
        let background = jobs.background("job_backgrounded").await;
        assert!(background.is_some());
        let result = waiter.await.unwrap().unwrap();
        match result {
            BlockResult::Backgrounded { info } => {
                assert_eq!(info.id, "job_backgrounded");
                assert_eq!(info.status, Status::Running);
            }
            other @ BlockResult::Finished { .. } => {
                panic!("expected backgrounded, got {other:?}")
            }
        }
        latch.notify_one();
        let result = jobs.wait("job_backgrounded", None).await;
        assert_eq!(result.info.unwrap().status, Status::Completed);
    }

    #[tokio::test]
    async fn job_persists_and_releases_durable_background_records() {
        let jobs = jobs().await;
        let notify = Arc::new(tokio::sync::Notify::new());
        let waiter = Arc::clone(&notify);
        let _ = jobs.start(StartInput {
            id: "job_bg".into(),
            type_: "shell".into(),
            title: Some("long".into()),
            metadata: Some(serde_json::json!({"sessionID": "ses_1"})),
            recovery: Some(Recovery::Shell {
                session_id: "ses_1".into(),
                shell_id: "sh_1".into(),
                command: "sleep".into(),
            }),
            notification_id: None,
            run: Box::pin(async move {
                waiter.notified().await;
                Ok("output".to_string())
            }),
        });
        let background = jobs.background("job_bg").await.unwrap();
        let notification_id = background.notification_id.clone().unwrap();
        let pending = jobs.pending_background().await;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, "job_bg");
        assert_eq!(pending[0].notification_id, notification_id);
        assert_eq!(
            pending[0].recovery,
            Recovery::Shell {
                session_id: "ses_1".into(),
                shell_id: "sh_1".into(),
                command: "sleep".into(),
            }
        );

        // notify_one (not notify_waiters): a permit survives being called
        // before the run future's first poll, so the single waiter cannot
        // miss the wakeup under scheduler jitter (the sibling latch tests
        // use the same pattern).
        notify.notify_one();
        let result = jobs.wait("job_bg", None).await;
        assert_eq!(result.info.unwrap().status, Status::Completed);
        // Record persists until the notification is admitted.
        assert_eq!(jobs.pending_background().await.len(), 1);
        jobs.complete_background(&notification_id).await;
        assert!(jobs.pending_background().await.is_empty());
    }

    #[tokio::test]
    async fn job_cancel_interrupts_and_marks_cancelled() {
        let jobs = jobs().await;
        let (_run, latch) = latch_run();
        let _ = jobs.start(StartInput {
            id: "job_cancel".into(),
            type_: "test".into(),
            title: None,
            metadata: None,
            recovery: None,
            notification_id: None,
            run: Box::pin(async {
                tokio::time::sleep(Duration::from_secs(60)).await;
                Ok("never".to_string())
            }),
        });
        let _ = latch;
        let info = jobs.cancel("job_cancel").await.unwrap();
        assert_eq!(info.status, Status::Cancelled);
        let result = jobs.wait("job_cancel", None).await;
        assert_eq!(result.info.unwrap().status, Status::Cancelled);
    }

    #[tokio::test]
    async fn job_background_all_targets_blocking_sessions() {
        let jobs = jobs().await;
        let (run, _) = latch_run();
        let _ = jobs.start(StartInput {
            id: "job_all".into(),
            type_: "shell".into(),
            title: None,
            metadata: None,
            recovery: None,
            notification_id: None,
            run,
        });
        let waiter = {
            let jobs = jobs.clone();
            tokio::spawn(async move { jobs.block("job_all", "ses_parent").await })
        };
        tokio::task::yield_now().await;
        let infos = jobs
            .background_all(BackgroundAllInput { session_id: "ses_parent", type_: Some("shell") })
            .await;
        assert_eq!(infos.len(), 1);
        assert!(matches!(waiter.await.unwrap().unwrap(), BlockResult::Backgrounded { .. }));
    }
}
