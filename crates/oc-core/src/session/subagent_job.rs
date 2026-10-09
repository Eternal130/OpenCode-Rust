//! `subagent-job.ts` port — the job runner + one-shot notification
//! observer for backgrounded subagents (core-runner-map §4.13).

use std::sync::Arc;

use tokio::sync::Mutex;

use crate::session::inbox::SessionInbox;
use crate::session::store::SessionStore;
use crate::subagent::{
    completion_text, BlockOutcome, ChildExecution, JobControl, JobRunOutcome, JobStart,
    SubagentRecovery,
};

/// `SubagentJob.make` — one observer per job generation, including
/// continuations of the same child (`notify` keys on
/// `"{childSessionID}:{startedAt}"`). Clone shares the notification set.
#[derive(Clone)]
pub struct SubagentJob {
    inner: Arc<SubagentJobInner>,
}

struct SubagentJobInner {
    store: SessionStore,
    inbox: SessionInbox,
    execution: Arc<dyn ChildExecution>,
    jobs: Arc<dyn JobControl>,
    notifications: Mutex<std::collections::BTreeSet<String>>,
}

impl SubagentJob {
    #[must_use]
    pub fn new(
        store: SessionStore,
        inbox: SessionInbox,
        execution: Arc<dyn ChildExecution>,
        jobs: Arc<dyn JobControl>,
    ) -> Self {
        Self {
            inner: Arc::new(SubagentJobInner {
                store,
                inbox,
                execution,
                jobs,
                notifications: Mutex::new(std::collections::BTreeSet::new()),
            }),
        }
    }

    fn clone_handle(&self) -> Self {
        self.clone()
    }

    /// `start` (subagent-job.ts:37-53): registers the child as a job whose
    /// body resumes the session and yields the newest completed assistant
    /// text. `Job.start` joining an already-running child skips the body,
    /// so the prompt admission happens outside the job.
    pub async fn start(&self, recovery: &SubagentRecovery) {
        let store = self.inner.store.clone();
        let execution = Arc::clone(&self.inner.execution);
        let child = recovery.child_session_id.clone();
        self.inner
            .jobs
            .start(JobStart {
                id: recovery.child_session_id.as_str().to_string(),
                job_type: "subagent".to_string(),
                title: Some(recovery.description.clone()),
                recovery: Some(recovery.clone()),
                run: Box::pin(async move {
                    match execution.resume(child.as_str()).await {
                        Ok(()) => {
                            JobRunOutcome::Completed(completion_text(&store, child.as_str()).await)
                        }
                        Err(message) => JobRunOutcome::Failed(message),
                    }
                }),
            })
            .await;
    }

    /// `background`: mark the child backgrounded, then arm the completion
    /// observer for this generation. The observer runs forked
    /// (`Effect.forkIn(scope)`): the launch path never waits on the
    /// notification.
    pub async fn background(&self, recovery: &SubagentRecovery) {
        if let Some(info) = self.inner.jobs.background(recovery.child_session_id.as_str()).await {
            self.spawn_notify(recovery, info.started_at);
        }
    }

    /// Forked notify (the `forkIn` half of subagent-job.ts:27-33).
    fn spawn_notify(&self, recovery: &SubagentRecovery, started_at: u64) {
        let this = self.clone_handle();
        let recovery = recovery.clone();
        tokio::spawn(async move {
            this.notify(&recovery, started_at).await;
        });
    }

    /// `notify` (subagent-job.ts:23-34): one observer per
    /// `{child}:{startedAt}` generation; when the job settles, deliver the
    /// synthetic completion into the parent.
    pub async fn notify(&self, recovery: &SubagentRecovery, started_at: u64) {
        let key = format!("{}:{started_at}", recovery.child_session_id);
        {
            let mut notifications = self.inner.notifications.lock().await;
            if notifications.contains(&key) {
                return;
            }
            notifications.insert(key.clone());
        }
        let info = self.inner.jobs.wait(recovery.child_session_id.as_str()).await;
        if let Some(info) = info {
            if info.status != crate::subagent::JobStatus::Running {
                super::subagent_completion::deliver(
                    &self.inner.inbox,
                    &self.inner.jobs,
                    super::subagent_completion::DeliverInput {
                        status: info.status,
                        output: info.output,
                        error: info.error,
                        notification_id: info.notification_id,
                        recovery: recovery.clone(),
                    },
                )
                .await
                .ok();
            }
        }
        self.inner.notifications.lock().await.remove(&key);
    }

    /// The foreground block path (subagent.ts:234-240): block on the child
    /// job; `on_interrupt` mirrors `Effect.onInterrupt` — the caller races
    /// this future against its interrupt token and the cancellation fires
    /// the child interrupt + job cancel.
    pub async fn block(&self, child: &str, session_id: &str) -> Option<BlockOutcome> {
        self.inner.jobs.block(child, session_id).await
    }

    /// Interrupt chain (subagent.ts:236-239): child session interrupt plus
    /// job cancel.
    pub async fn interrupt(&self, child: &str) {
        self.inner.execution.interrupt(child).await;
        self.inner.jobs.cancel(child).await;
    }
}
