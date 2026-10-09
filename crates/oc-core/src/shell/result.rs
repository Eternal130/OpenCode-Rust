//! Port of `packages/core/src/shell/result.ts` — terminal-state shaping and
//! the exact notification wrapper formats.

use oc_schema::shell::{Info, Status};

/// `ShellResult.Result`.
#[derive(Debug, Clone)]
pub struct ShellResult {
    pub info: Info,
    pub capture: Option<Capture>,
}

/// The bounded tail capture of a finished shell command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capture {
    pub output: String,
    pub truncated: bool,
}

/// `ShellResult.Output`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Output {
    pub output: String,
    pub truncated: bool,
    pub exit: Option<f64>,
    pub signal: Option<String>,
    pub timeout: Option<bool>,
}

pub const UNAVAILABLE: &str = "Shell command output is no longer available.";

/// `ShellResult.output`.
#[must_use]
pub fn output(result: &ShellResult) -> Output {
    Output {
        output: result
            .capture
            .as_ref()
            .map_or_else(|| UNAVAILABLE.to_string(), |c| c.output.clone()),
        truncated: result.capture.as_ref().is_some_and(|c| c.truncated),
        exit: result.info.exit.map(|n| n.0),
        signal: result.info.signal.clone(),
        timeout: (result.info.status == Status::Timeout).then_some(true),
    }
}

/// `ShellResult.notice` — the completion notice line.
#[must_use]
pub fn notice(output: &Output) -> Option<String> {
    if output.timeout == Some(true) {
        return Some("Timed out before completion".into());
    }
    if let Some(signal) = &output.signal {
        return Some(format!("Killed by {signal}"));
    }
    if let Some(exit) = output.exit {
        if exit != 0.0 {
            return Some(format!("Exited with code {}", format_exit(exit)));
        }
    }
    None
}

#[allow(clippy::cast_possible_truncation)] // parity: TS number→string coercion truncates
fn format_exit(exit: f64) -> String {
    if exit.fract() == 0.0 {
        format!("{}", exit as i64)
    } else {
        format!("{exit}")
    }
}

/// `ShellResult.metadata`.
#[must_use]
pub fn metadata(output: &Output) -> serde_json::Value {
    let mut value = serde_json::json!({ "truncated": output.truncated });
    if let Some(exit) = output.exit {
        value["exit"] = serde_json::json!(exit);
    }
    if let Some(signal) = &output.signal {
        value["signal"] = serde_json::json!(signal);
    }
    if let Some(timeout) = output.timeout {
        value["timeout"] = serde_json::json!(timeout);
    }
    value
}

/// `ShellResult.notification` — the `<shell …>` wrapper.
#[must_use]
pub fn notification(input: NotificationInput<'_>) -> Notification {
    let id = input.job_id.unwrap_or(input.shell_id);
    let mut meta = serde_json::json!({
        "source": "shell",
        "shellID": input.shell_id,
        "state": input.state,
    });
    if let Some(job_id) = input.job_id {
        meta["jobID"] = serde_json::json!(job_id);
    }
    if let Some(output) = input.output {
        if let serde_json::Value::Object(map) = metadata(output) {
            for (key, value) in map {
                meta[&key] = value;
            }
        }
    }
    Notification {
        text: format!(
            "<shell id=\"{}\" state=\"{}\" command=\"{}\">\n{}\n</shell>",
            id, input.state, input.command, input.text
        ),
        metadata: meta,
    }
}

/// `ShellResult.userNotification` — the `!` command wrapper.
#[must_use]
pub fn user_notification(result: &ShellResult) -> Notification {
    let captured = output(result);
    let status = if result.info.status == Status::Killed {
        Some("Cancelled".to_string())
    } else if captured.exit == Some(0.0) {
        None
    } else {
        Some(notice(&captured).unwrap_or_else(|| "Exited with code unknown".into()))
    };
    let text = [Some(captured.output.clone()), status]
        .into_iter()
        .flatten()
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    let message = notification(NotificationInput {
        shell_id: result.info.id.as_str(),
        job_id: None,
        command: &result.info.command,
        state: if result.info.status == Status::Killed { "cancelled" } else { "completed" },
        text: &text,
        output: Some(&captured),
    });
    Notification {
        text: format!("The following shell command was executed by the user:\n{}", message.text),
        metadata: message.metadata,
    }
}

/// Input of [`notification`].
#[derive(Debug, Clone, Copy)]
pub struct NotificationInput<'a> {
    pub shell_id: &'a str,
    pub job_id: Option<&'a str>,
    pub command: &'a str,
    pub state: &'a str,
    pub text: &'a str,
    pub output: Option<&'a Output>,
}

/// One notification message (text + metadata).
#[derive(Debug, Clone, PartialEq)]
pub struct Notification {
    pub text: String,
    pub metadata: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_output(exit: Option<f64>, signal: Option<&str>, timeout: bool) -> Output {
        Output {
            output: "body".into(),
            truncated: false,
            exit,
            signal: signal.map(String::from),
            timeout: timeout.then_some(true),
        }
    }

    #[test]
    fn shell_result_notice_variants() {
        assert_eq!(
            notice(&make_output(None, None, true)).as_deref(),
            Some("Timed out before completion")
        );
        assert_eq!(
            notice(&make_output(None, Some("SIGKILL"), false)).as_deref(),
            Some("Killed by SIGKILL")
        );
        assert_eq!(
            notice(&make_output(Some(7.0), None, false)).as_deref(),
            Some("Exited with code 7")
        );
        assert_eq!(notice(&make_output(Some(0.0), None, false)), None);
    }

    #[test]
    fn shell_result_notification_wrapper() {
        let message = notification(NotificationInput {
            shell_id: "sh_1",
            job_id: Some("job_1"),
            command: "printf hi",
            state: "completed",
            text: "hi",
            output: Some(&make_output(Some(0.0), None, false)),
        });
        assert_eq!(
            message.text,
            "<shell id=\"job_1\" state=\"completed\" command=\"printf hi\">\nhi\n</shell>"
        );
        assert_eq!(message.metadata["shellID"], "sh_1");
        assert_eq!(message.metadata["jobID"], "job_1");
        assert_eq!(message.metadata["state"], "completed");
        assert_eq!(message.metadata["exit"].as_f64(), Some(0.0));
    }

    #[test]
    fn shell_result_user_notification_wrapper() {
        let info = Info {
            id: oc_schema::id::ShellId("sh_1".to_string()),
            status: Status::Exited,
            command: "printf hi".into(),
            cwd: "/repo".into(),
            shell: "/bin/bash".into(),
            file: "/tmp/out".into(),
            pid: None,
            exit: Some(oc_schema::common::Num(1.0)),
            signal: None,
            metadata: std::collections::BTreeMap::default(),
            time: oc_schema::shell::Time { started: 0, completed: Some(1) },
        };
        let message = user_notification(&ShellResult {
            info,
            capture: Some(Capture { output: "hi".into(), truncated: false }),
        });
        assert_eq!(
            message.text,
            "The following shell command was executed by the user:\n<shell id=\"sh_1\" state=\"completed\" command=\"printf hi\">\nhi\n\nExited with code 1\n</shell>"
        );
    }
}
