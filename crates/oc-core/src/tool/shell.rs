//! Port of `packages/core/src/tool/plugin/shell.ts` — the shell tool (§4.4):
//! permission assert over the parsed command resources, detached execution
//! through the [`Shell`] service, Job wrapping, foreground/background
//! semantics and every notification format.

use std::collections::HashMap;
use std::sync::Arc;

use crate::env::SessionEnv;
use crate::jobs::{BlockResult, Jobs, Recovery, RunError, StartInput};
use crate::permission::{AssertInput, PermissionError};
use crate::shell::{result as shell_result, select, Shell};
use crate::tool::{
    permission_cause, BuiltinTool, ToolExecuteWithContext, ToolFailure, ToolOutcome, ToolServices,
};

/// `ShellTool.DEFAULT_TIMEOUT_MS`.
pub const DEFAULT_TIMEOUT_MS: u64 = 2 * 60 * 1_000;

pub const BACKGROUND_INSTRUCTION: &str = "You will be notified automatically when the command finishes. The notification will include the command's output. Unless the user explicitly asks otherwise, DO NOT poll for completion, even if you need the final result to continue. Repeatedly sleeping and reading or searching the output file is polling, not useful work. You may read the current output if it lets you do useful work now, but do not repeatedly check it while waiting for the command to finish. Keep working on anything that does not depend on the result. If you have nothing else to do, end your response; you will be resumed automatically when the command finishes.";

pub fn description(shell: Option<&str>) -> String {
    let os = if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(target_os = "windows") {
        "Windows"
    } else if cfg!(target_os = "linux") {
        "Linux"
    } else {
        "unknown"
    };
    [
        Some("Execute a shell command and return its output.".to_string()),
        shell.map(|shell| format!("Commands run on {os} using {shell}.")),
        Some("Quote file paths containing spaces or special characters.".to_string()),
        Some("Prefer dedicated tools over shell commands when possible.".to_string()),
        Some("When output is large, the full result is saved to a file and a truncated preview is returned.".to_string()),
        Some("Rely on automatic truncation unless filtering the output is more useful.".to_string()),
        Some("Commands accept an optional timeout, background commands have no timeout by default.".to_string()),
        Some("Background commands return immediately, and you will be notified when they complete.".to_string()),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ")
}

/// The shell tool dependencies.
pub struct ShellToolServices {
    pub shell: Shell,
    pub jobs: Jobs,
    pub env: SessionEnv,
    /// `experimental.portable_shell_scanner`.
    pub portable_scanner: bool,
    /// Config `shell` — the `ShellSelect` "config"-priority input for
    /// the tool's compat resolve.
    pub config_shell: Option<String>,
    /// Emits the synthetic notification into the owning session
    /// (`sessions.synthetic`).
    pub synthetic: Arc<dyn SyntheticPublisher>,
}

/// `sessions.synthetic` seam — publishes the `<shell …>` notification into
/// the session transcript.
pub trait SyntheticPublisher: Send + Sync {
    fn publish(
        &self,
        session_id: &str,
        description: &str,
        text: &str,
        metadata: serde_json::Value,
        notification_id: Option<&str>,
    );
}

/// A no-op publisher (tests that only check the tool result).
#[derive(Default)]
pub struct NoopSynthetic;

impl SyntheticPublisher for NoopSynthetic {
    fn publish(
        &self,
        _session_id: &str,
        _description: &str,
        _text: &str,
        _metadata: serde_json::Value,
        _notification_id: Option<&str>,
    ) {
    }
}

/// `ShellTool.Output`.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct ShellToolOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell_id: Option<String>,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<bool>,
    pub output: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

/// `resultMessages` — output plus notice as separate text parts.
pub fn result_messages(output: &ShellToolOutput) -> Vec<String> {
    let notice = if output.status.as_deref() == Some("running") {
        Some(BACKGROUND_INSTRUCTION.to_string())
    } else {
        shell_result::notice(&shell_result::Output {
            output: String::new(),
            truncated: false,
            exit: output.exit,
            signal: output.signal.clone(),
            timeout: output.timeout,
        })
    };
    [(!output.output.is_empty()).then(|| output.output.clone()), notice]
        .into_iter()
        .flatten()
        .collect()
}

/// `backgroundResult`.
fn background_result(shell_id: &str, file: &str) -> ShellToolOutput {
    ShellToolOutput {
        output: format!("Command moved to the background (shell ID: {shell_id}).\nOutput is streaming to: {file}"),
        shell_id: Some(shell_id.to_string()),
        truncated: false,
        status: Some("running".into()),
        exit: None,
        signal: None,
        timeout: None,
    }
}

/// `ShellTool.prepare` — external-directory authorization + command parsing
/// + permission assert. Returns the invocation timeout.
async fn prepare(
    services: &ToolServices,
    invocation: &mut crate::shell::Invocation,
    context: &crate::location::ToolCallContext,
    portable: bool,
) -> Result<u64, ToolFailure> {
    let target = services.access.resolve(&crate::file_access::ResolveInput {
        path: invocation.cwd.clone(),
        kind: Some(crate::file_access::Kind::Directory),
    });
    invocation.cwd = target.absolute.to_string_lossy().to_string();
    let timeout = invocation.timeout;
    let parsed = crate::shell::parse::scan(
        &invocation.command,
        &invocation.shell,
        &target.absolute,
        portable,
    )
    .map_err(|error| ToolFailure {
        message: format!("Unable to execute command: {}", invocation.command),
        cause: Some(super::FailureCause::Nested(error)),
    })?;
    let mut targets = vec![target.clone()];
    for directory in &parsed.directories {
        targets.push(
            services.access.resolve(&crate::file_access::ResolveInput {
                path: crate::file_access::resolve_path(&target.absolute, directory, &services.home)
                    .to_string_lossy()
                    .to_string(),
                kind: Some(crate::file_access::Kind::Directory),
            }),
        );
    }
    let invocation_context = crate::file_access::Invocation {
        session_id: context.session_id.clone(),
        agent: Some(context.agent.clone()),
        message_id: context.message_id.clone(),
        id: context.call_id.clone(),
    };
    services.access.authorize_external(&targets, &invocation_context, None).await.map_err(
        |error: PermissionError| ToolFailure {
            message: format!("Unable to execute command: {}", invocation.command),
            cause: Some(permission_cause(&error)),
        },
    )?;
    if !parsed.commands.is_empty() {
        services
            .permission
            .assert(AssertInput {
                id: None,
                session_id: context.session_id.clone(),
                agent: Some(context.agent.clone()),
                action: "shell".into(),
                resources: parsed.commands.iter().map(|command| command.resource.clone()).collect(),
                save: Some(parsed.commands.iter().map(|command| command.save.clone()).collect()),
                metadata: None,
                source: Some(oc_schema::permission::Source::Tool {
                    message_id: context.message_id.clone(),
                    id: context.call_id.clone(),
                }),
            })
            .await
            .map_err(|error: PermissionError| ToolFailure {
                message: format!("Unable to execute command: {}", invocation.command),
                cause: Some(permission_cause(&error)),
            })?;
    }
    // Approval can outlive the directory, so validate immediately before
    // spawning.
    let workdir = std::fs::metadata(&invocation.cwd);
    match workdir {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => {
            return Err(ToolFailure {
                message: format!("Unable to execute command: {}", invocation.command),
                cause: Some(super::FailureCause::Nested(format!(
                    "Working directory is not a directory: {}",
                    invocation.cwd
                ))),
            })
        }
        Err(_) => {
            return Err(ToolFailure {
                message: format!("Unable to execute command: {}", invocation.command),
                cause: Some(super::FailureCause::Nested(format!(
                    "Working directory does not exist: {}",
                    invocation.cwd
                ))),
            })
        }
    }
    Ok(timeout)
}

/// The shell leaf executor.
pub struct ShellExecute {
    pub deps: Arc<ShellToolServices>,
}

#[derive(serde::Deserialize)]
struct Input {
    command: String,
    workdir: Option<String>,
    timeout: Option<u64>,
    background: Option<bool>,
}

impl ToolExecuteWithContext for ShellExecute {
    fn execute<'a>(
        &'a self,
        services: &'a ToolServices,
        input: &'a serde_json::Value,
        context: &'a crate::location::ToolCallContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ToolOutcome, ToolFailure>> + Send + 'a>,
    > {
        let input = input.clone();
        let context = context.clone();
        let deps = Arc::clone(&self.deps);
        Box::pin(async move {
            let input: Input = serde_json::from_value(input)
                .map_err(|error| ToolFailure::message(format!("Invalid arguments: {error}")))?;
            let background = input.background.unwrap_or(false);
            let timeout = if background {
                input.timeout.unwrap_or(0)
            } else {
                input.timeout.unwrap_or(DEFAULT_TIMEOUT_MS)
            };

            // Build the invocation (session env + markers), dispatch
            // `shell.create.before`, layer the agent markers, run the
            // permission pipeline over the FINAL invocation, then spawn
            // (v2 ordering: hooks.trigger before the `before` closure,
            // shell.ts:274-275).
            let session_env = deps.env.get(context.session_id.as_str());
            let mut env: HashMap<String, String> =
                session_env.unwrap_or_else(|| std::env::vars().collect());
            for (key, value) in crate::shell::TERMINAL_ENV {
                env.insert((*key).to_string(), (*value).to_string());
            }
            let compatible_shell = select::resolve(deps.config_shell.as_deref(), true);
            let mut invocation = crate::shell::Invocation {
                cwd: input
                    .workdir
                    .clone()
                    .unwrap_or_else(|| services.directory.to_string_lossy().to_string()),
                command: input.command.clone(),
                timeout,
                shell: compatible_shell,
                env,
            };
            deps.shell.trigger_create_before(&mut invocation).await.map_err(|error| {
                ToolFailure {
                    message: format!("Unable to execute command: {}", input.command),
                    cause: Some(super::FailureCause::Nested(error.to_string())),
                }
            })?;
            invocation.env.insert("AGENT".into(), "1".into());
            invocation.env.insert("OPENCODE".into(), "1".into());
            invocation.env.entry("AI_AGENT".to_string()).or_insert_with(|| "opencode".to_string());
            invocation.env.insert("OPENCODE_SESSION_ID".into(), context.session_id.to_string());
            let final_timeout =
                prepare(services, &mut invocation, &context, deps.portable_scanner).await?;
            invocation.timeout = final_timeout;
            let info = deps
                .shell
                .create_invocation(
                    invocation,
                    serde_json::from_value(
                        serde_json::json!({ "sessionID": context.session_id.as_str() }),
                    )
                    .unwrap_or_default(),
                )
                .await
                .map_err(|error| ToolFailure {
                    message: format!("Unable to execute command: {}", input.command),
                    cause: Some(super::FailureCause::Nested(error.to_string())),
                })?;

            // The run body settles into the job carrying the structured
            // output as JSON (notifications and the foreground result both
            // decode it).
            let shell = deps.shell.clone();
            let info_for_run = info.clone();
            let final_timeout_for_run = final_timeout;
            let shell_id = info.id.to_string();
            let run: crate::jobs::Run = Box::pin(async move {
                let result = shell.result(info_for_run).await;
                let Some(capture) = result.capture else {
                    return Err(RunError::message("Shell command output is no longer available."));
                };
                let output = shell_result::output(&shell_result::ShellResult {
                    info: result.info.clone(),
                    capture: Some(capture),
                });
                let mut tool_output = ShellToolOutput {
                    exit: output.exit,
                    signal: output.signal.clone(),
                    shell_id: Some(shell_id),
                    truncated: output.truncated,
                    timeout: output.timeout,
                    output: output.output.clone(),
                    status: Some("completed".into()),
                };
                if output.timeout == Some(true) {
                    tool_output.output = format!(
                        "{}\n\nCommand exceeded timeout of {final_timeout_for_run} ms. Retry with a larger timeout if the command is expected to take longer.",
                        output.output
                    );
                }
                serde_json::to_string(&tool_output)
                    .map_err(|_| RunError::message("encoding failure"))
            });

            let job = deps.jobs.start(StartInput {
                // CodeMode children share a tool-call ID, but each shell must
                // own its job.
                id: info.id.to_string(),
                type_: "shell".into(),
                title: Some(info.command.clone()),
                metadata: Some(serde_json::json!({
                    "sessionID": context.session_id.as_str(),
                    "shellID": info.id.as_str(),
                })),
                recovery: Some(Recovery::Shell {
                    session_id: context.session_id.to_string(),
                    shell_id: info.id.to_string(),
                    command: info.command.clone(),
                }),
                notification_id: None,
                run,
            });

            // `Effect.onInterrupt` (shell.ts:234,259): dropping this future
            // mid-run — the runner's tool-fiber bell — removes the process
            // and cancels the job. Disarmed on every completing return.
            let mut cancel_guard = ShellCancelGuard {
                shell: deps.shell.clone(),
                jobs: deps.jobs.clone(),
                shell_id: info.id.clone(),
                job_id: job.id.clone(),
                armed: true,
            };

            let notify_when_done =
                |job_id: String, shell_info: oc_schema::shell::Info, command: String| {
                    let jobs = deps.jobs.clone();
                    let synthetic = Arc::clone(&deps.synthetic);
                    let session_id = context.session_id.to_string();
                    tokio::spawn(async move {
                        let wait = jobs.wait(&job_id, None).await;
                        let Some(job_info) = wait.info else { return };
                        if job_info.status == crate::jobs::Status::Running {
                            return;
                        }
                        let state = match job_info.status {
                            crate::jobs::Status::Completed => "completed",
                            crate::jobs::Status::Error => "error",
                            crate::jobs::Status::Cancelled => "cancelled",
                            crate::jobs::Status::Running => return,
                        };
                        let text = match (&job_info.output, state) {
                            (Some(payload), "completed") => decode_messages(payload),
                            (Some(payload), _) => payload.clone(),
                            (None, "error") => {
                                job_info.error.clone().unwrap_or_else(|| "Command failed".into())
                            }
                            (None, _) => "Cancelled".to_string(),
                        };
                        let notification =
                            shell_result::notification(shell_result::NotificationInput {
                                shell_id: shell_info.id.as_str(),
                                job_id: Some(&job_id),
                                command: &command,
                                state,
                                text: &text,
                                output: None,
                            });
                        synthetic.publish(
                            &session_id,
                            &command,
                            &notification.text,
                            notification.metadata,
                            job_info.notification_id.as_deref(),
                        );
                        if let Some(notification_id) = &job_info.notification_id {
                            jobs.complete_background(notification_id).await;
                        }
                    });
                };

            if background {
                deps.jobs.background(&job.id).await;
                notify_when_done(job.id.clone(), info.clone(), info.command.clone());
                let output = background_result(info.id.as_str(), &info.file);
                cancel_guard.disarm();
                return Ok(tool_result(&output));
            }

            let result = deps.jobs.block(&job.id, context.session_id.as_str()).await;
            cancel_guard.disarm();
            match result {
                Some(BlockResult::Backgrounded { .. }) => {
                    deps.shell.timeout(&info.id, 0).await.ok();
                    notify_when_done(job.id.clone(), info.clone(), info.command.clone());
                    let output = background_result(info.id.as_str(), &info.file);
                    Ok(tool_result(&output))
                }
                Some(BlockResult::Finished { info })
                    if info.status == crate::jobs::Status::Error =>
                {
                    Err(ToolFailure {
                        message: format!("Unable to execute command: {}", input.command),
                        cause: Some(super::FailureCause::Nested(
                            info.error.unwrap_or_else(|| "Command failed".into()),
                        )),
                    })
                }
                Some(BlockResult::Finished { info })
                    if info.status == crate::jobs::Status::Cancelled =>
                {
                    Err(ToolFailure {
                        message: format!("Unable to execute command: {}", input.command),
                        cause: Some(super::FailureCause::Nested("Command cancelled".into())),
                    })
                }
                Some(BlockResult::Finished { info }) => {
                    // Foreground completion: decode the structured output the
                    // run body settled with.
                    let output: ShellToolOutput = info
                        .output
                        .as_deref()
                        .and_then(|payload| serde_json::from_str(payload).ok())
                        .unwrap_or_default();
                    Ok(tool_result(&output))
                }
                None => Err(ToolFailure {
                    message: format!("Unable to execute command: {}", input.command),
                    cause: Some(super::FailureCause::Nested("job missing".into())),
                }),
            }
        })
    }
}

fn decode_messages(payload: &str) -> String {
    serde_json::from_str::<ShellToolOutput>(payload)
        .map(|output| result_messages(&output).join("\n\n"))
        .unwrap_or_else(|_| payload.to_string())
}

/// The `Effect.onInterrupt` translation: future-drop (the runner's
/// tool-fiber cancel) fires the same cleanup v2 runs on fiber interrupt —
/// remove the process, cancel the job. Fire-and-forget (Drop cannot
/// await); both operations are idempotent.
struct ShellCancelGuard {
    shell: crate::shell::Shell,
    jobs: crate::jobs::Jobs,
    shell_id: oc_schema::id::ShellId,
    job_id: String,
    armed: bool,
}

impl ShellCancelGuard {
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ShellCancelGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let shell = self.shell.clone();
        let jobs = self.jobs.clone();
        let shell_id = self.shell_id.clone();
        let job_id = self.job_id.clone();
        tokio::spawn(async move {
            let _ = shell.remove(&shell_id).await;
            jobs.cancel(&job_id).await;
        });
    }
}

fn tool_result(output: &ShellToolOutput) -> ToolOutcome {
    let content = result_messages(output)
        .into_iter()
        .map(|text| oc_schema::tool::Content::Text { text })
        .collect();
    let mut metadata = serde_json::json!({ "status": output.status });
    if output.truncated {
        metadata["truncated"] = serde_json::json!(true);
    }
    if let Some(exit) = output.exit {
        metadata["exit"] = serde_json::json!(exit);
    }
    if let Some(shell_id) = &output.shell_id {
        metadata["shellID"] = serde_json::json!(shell_id);
    }
    ToolOutcome {
        content,
        metadata: Some(serde_json::from_value(metadata).unwrap_or_default()),
        output: Some(serde_json::to_value(output).unwrap_or_default()),
    }
}

/// The shell builtin registration.
pub fn tool(deps: Arc<ShellToolServices>) -> BuiltinTool {
    let shell_path = select::resolve(deps.config_shell.as_deref(), true);
    BuiltinTool {
        name: "shell",
        description: description(Some(select::name(&shell_path).as_str())),
        input: serde_json::json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "Shell command string to execute" },
                "workdir": { "type": "string", "description": "Working directory to execute the command in. Defaults to the current working directory. When possible, avoid changing directories in the command and set the working directory here instead." },
                "timeout": { "type": "integer", "minimum": 0, "description": "Timeout in milliseconds. Set to 0 to disable the timeout. Defaults to 120000 for foreground commands. Background commands have no timeout by default." },
                "background": { "type": "boolean", "description": "Run the command in the background and return immediately (useful for dev servers and long-running builds). You do not need to use '&' at the end of the command when using this parameter. You will be notified when it completes. DO NOT poll for completion." }
            },
            "required": ["command"]
        }),
        permission: Some("shell"),
        codemode: false,
        namespace: None,
        execute: Arc::new(ShellExecute { deps }),
    }
}

/// The user `!` command path (same shape with timeout 0 + background).
pub async fn user_command(
    deps: &Arc<ShellToolServices>,
    services: &ToolServices,
    session_id: &str,
    command: &str,
) -> Result<shell_result::Notification, ToolFailure> {
    let shell_path = select::resolve(deps.config_shell.as_deref(), true);
    let mut env: HashMap<String, String> =
        deps.env.get(session_id).unwrap_or_else(|| std::env::vars().collect());
    for (key, value) in crate::shell::TERMINAL_ENV {
        env.insert((*key).to_string(), (*value).to_string());
    }
    let mut invocation = crate::shell::Invocation {
        command: command.to_string(),
        cwd: services.directory.to_string_lossy().to_string(),
        timeout: 0,
        shell: shell_path,
        env,
    };
    // The user `!` path goes through `Shell.create` in v2 — the
    // `shell.create.before` dispatch applies here too.
    deps.shell.trigger_create_before(&mut invocation).await.map_err(|error| ToolFailure {
        message: format!("Unable to execute command: {command}"),
        cause: Some(super::FailureCause::Nested(error.to_string())),
    })?;
    invocation.env.insert("AGENT".into(), "1".into());
    invocation.env.insert("OPENCODE".into(), "1".into());
    invocation.env.entry("AI_AGENT".to_string()).or_insert_with(|| "opencode".to_string());
    invocation.env.insert("OPENCODE_SESSION_ID".into(), session_id.to_string());
    let context = crate::location::ToolCallContext {
        session_id: oc_schema::id::SessionId(session_id.to_string()),
        agent: oc_schema::id::AgentId("code".to_string()),
        message_id: String::new(),
        call_id: String::new(),
    };
    let prepared = prepare(services, &mut invocation, &context, deps.portable_scanner).await?;
    invocation.timeout = prepared;
    let started = deps
        .shell
        .create_invocation(
            invocation,
            serde_json::from_value(serde_json::json!({ "sessionID": session_id }))
                .unwrap_or_default(),
        )
        .await
        .map_err(|error| ToolFailure {
            message: format!("Unable to execute command: {command}"),
            cause: Some(super::FailureCause::Nested(error.to_string())),
        })?;
    let result = deps.shell.result(started).await;
    Ok(shell_result::user_notification(&result))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_shell_result_messages_compose_output_and_notice() {
        let output = ShellToolOutput {
            output: "hello".into(),
            exit: Some(0.0),
            truncated: false,
            status: Some("completed".into()),
            shell_id: None,
            signal: None,
            timeout: None,
        };
        assert_eq!(result_messages(&output), vec!["hello"]);

        let failed = ShellToolOutput {
            exit: Some(7.0),
            output: "body".into(),
            status: Some("completed".into()),
            shell_id: None,
            signal: None,
            timeout: None,
            truncated: false,
        };
        assert_eq!(result_messages(&failed), vec!["body", "Exited with code 7"]);

        let running = ShellToolOutput {
            status: Some("running".into()),
            output: "streaming".into(),
            exit: None,
            truncated: false,
            shell_id: None,
            signal: None,
            timeout: None,
        };
        assert_eq!(result_messages(&running), vec!["streaming", BACKGROUND_INSTRUCTION]);
    }

    #[test]
    fn tool_shell_background_result_format() {
        let output = background_result("sh_1", "/data/shell/p/sh_1.out");
        assert_eq!(
            output.output,
            "Command moved to the background (shell ID: sh_1).\nOutput is streaming to: /data/shell/p/sh_1.out"
        );
        assert_eq!(output.status.as_deref(), Some("running"));
        assert_eq!(output.shell_id.as_deref(), Some("sh_1"));
    }

    #[test]
    fn tool_shell_description_mentions_shell_and_os() {
        let description = description(Some("/bin/bash"));
        assert!(description.contains("Commands run on"));
        assert!(description.contains("/bin/bash"));
        assert!(description.contains("Prefer dedicated tools over shell commands when possible."));
    }

    #[test]
    fn tool_shell_timeout_notice_appends_to_output() {
        let payload = r#"{"output":"body\n\nCommand exceeded timeout of 100 ms. Retry with a larger timeout if the command is expected to take longer.","truncated":false,"timeout":true,"status":"completed"}"#;
        assert!(decode_messages(payload).contains("Command exceeded timeout of 100 ms."));
    }
}
