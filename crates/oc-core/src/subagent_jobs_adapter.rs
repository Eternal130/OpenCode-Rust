//! Adapter from the subagent lane's [`JobControl`] seam onto the C2
//! `jobs.rs` port (`crate::jobs::Jobs`), which landed while Wave D was in
//! flight. Keeps the subagent modules decoupled from the jobs internals.

use std::sync::Arc;

use oc_schema::id::MessageId;

use crate::jobs::{self, Recovery, RunError};

use crate::subagent::{
    BlockOutcome, JobControl, JobInfo, JobRun, JobRunOutcome, JobStart, JobStatus, SubagentRecovery,
};

/// Wraps the real jobs service behind the subagent seam.
#[derive(Clone)]
pub struct JobsAdapter(pub Arc<jobs::Jobs>);

fn to_recovery(recovery: Option<&SubagentRecovery>) -> Option<Recovery> {
    recovery.map(|recovery| Recovery::Subagent {
        parent_session_id: recovery.parent_session_id.as_str().to_string(),
        child_session_id: recovery.child_session_id.as_str().to_string(),
        agent: recovery.agent.clone(),
        description: recovery.description.clone(),
    })
}

fn to_info(info: jobs::Info) -> JobInfo {
    JobInfo {
        id: info.id,
        job_type: info.type_,
        title: info.title,
        status: match info.status {
            jobs::Status::Running => JobStatus::Running,
            jobs::Status::Completed => JobStatus::Completed,
            jobs::Status::Error => JobStatus::Error,
            jobs::Status::Cancelled => JobStatus::Cancelled,
        },
        started_at: info.started_at,
        completed_at: info.completed_at,
        output: info.output,
        error: info.error,
        notification_id: info
            .notification_id
            .map(|id| MessageId::parse(id).unwrap_or_else(|err| MessageId(err.value))),
    }
}

#[async_trait::async_trait]
impl JobControl for JobsAdapter {
    async fn start(&self, input: JobStart) -> JobInfo {
        let run: jobs::Run = Box::pin(async move {
            match input_run(input.run).await {
                JobRunOutcome::Completed(output) => Ok(output),
                JobRunOutcome::Failed(message) => Err(RunError::message(message)),
                JobRunOutcome::Cancelled => {
                    Err(RunError { message: "cancelled".into(), interrupted: true })
                }
            }
        });
        let info = self.0.start(jobs::StartInput {
            id: input.id,
            type_: input.job_type,
            title: input.title,
            metadata: None,
            recovery: to_recovery(input.recovery.as_ref()),
            notification_id: None,
            run,
        });
        to_info(info)
    }

    async fn block(&self, id: &str, session_id: &str) -> Option<BlockOutcome> {
        match self.0.block(id, session_id).await? {
            jobs::BlockResult::Finished { info } => Some(BlockOutcome::Finished(to_info(info))),
            jobs::BlockResult::Backgrounded { info } => {
                Some(BlockOutcome::Backgrounded(to_info(info)))
            }
        }
    }

    async fn background(&self, id: &str) -> Option<JobInfo> {
        self.0.background(id).await.map(to_info)
    }

    async fn cancel(&self, id: &str) -> Option<JobInfo> {
        self.0.cancel(id).await.map(to_info)
    }

    async fn wait(&self, id: &str) -> Option<JobInfo> {
        self.0.wait(id, None).await.info.map(to_info)
    }

    async fn get(&self, id: &str) -> Option<JobInfo> {
        self.0.get(id).map(to_info)
    }

    async fn complete_background(&self, notification_id: &str) {
        self.0.complete_background(notification_id).await;
    }
}

async fn input_run(run: JobRun) -> JobRunOutcome {
    run.await
}
