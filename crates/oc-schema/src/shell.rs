//! Port of `shell.ts`.

use serde::{Deserialize, Serialize};

use crate::common::{NonNegativeInt, Record};
use crate::id::ShellId;

/// `Shell.Status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Running,
    Exited,
    Timeout,
    Killed,
}

/// `Shell.Time` — epoch milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Time {
    /// Start time in milliseconds since the Unix epoch.
    pub started: u64,
    /// Completion time in milliseconds since the Unix epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed: Option<u64>,
}

/// `Shell.Info`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Info {
    pub id: ShellId,
    pub status: Status,
    pub command: String,
    pub cwd: String,
    pub shell: String,
    /// Absolute path of the file capturing combined stdout/stderr.
    pub file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<NonNegativeInt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit: Option<crate::common::Num>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<String>,
    /// Opaque caller-supplied tags; always present, defaults to `{}`.
    #[serde(default)]
    pub metadata: Record,
    pub time: Time,
}

/// `Shell.CreateInput`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CreateInput {
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<NonNegativeInt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Record>,
}

/// `Shell.OutputInput`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct OutputInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<NonNegativeInt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<NonNegativeInt>,
}

/// `Shell.Output` — one page of captured output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Output {
    pub output: String,
    /// Absolute cursor after this page; equals `size` once caught up.
    pub cursor: NonNegativeInt,
    /// Total bytes captured so far.
    pub size: NonNegativeInt,
    pub truncated: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_wire() {
        let info = Info {
            id: ShellId::parse("sh_1").unwrap(),
            status: Status::Exited,
            command: "ls".into(),
            cwd: "/repo".into(),
            shell: "bash".into(),
            file: "/tmp/out".into(),
            pid: Some(42),
            exit: Some(crate::common::Num(0.0)),
            signal: None,
            metadata: Record::default(),
            time: Time { started: 100, completed: Some(200) },
        };
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "id": "sh_1",
                "status": "exited",
                "command": "ls",
                "cwd": "/repo",
                "shell": "bash",
                "file": "/tmp/out",
                "pid": 42,
                "exit": 0,
                "metadata": {},
                "time": {"started": 100, "completed": 200}
            })
        );
        let back: Info = serde_json::from_value(json).unwrap();
        assert_eq!(back, info);
    }
}
