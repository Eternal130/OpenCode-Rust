//! Port of `tool/plugin/glob.ts` — the file-path search leaf spawning
//! `rg --files` with the exact v2 argument construction (core-runner-map
//! §4.7). The runner lives in [`super::grep`].

use std::path::Path;

use oc_schema::tool::Content;
use serde_json::Value;

use super::grep::{ripgrep_glob, DEFAULT_SEARCH_LIMIT};
use super::{BuiltinTool, Context, ToolFailure, ToolOutcome, ToolServices};
use crate::file_access::{Invocation, Kind, ResolveInput};

pub fn description() -> String {
    "Search file paths using a glob pattern (examples: \"**/*.ts\", \"src/**/*.tsx\").".to_string()
}

pub fn schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "pattern": { "type": "string", "description": "Glob pattern to match files against" },
            "path": { "type": "string", "description": "Directory to search. Defaults to the current working directory." },
            "hidden": { "type": "boolean", "description": "Include hidden files and directories (default: false)." },
            "limit": { "type": "integer", "minimum": 1, "description": "Maximum number of matching files to return (default: 100)" }
        },
        "required": ["pattern"]
    })
}

pub fn tool(_services: &std::sync::Arc<ToolServices>) -> BuiltinTool {
    BuiltinTool {
        name: "glob",
        description: description(),
        input: schema(),
        permission: None,
        codemode: false,
        namespace: None,
        execute: std::sync::Arc::new(GlobExecutor),
    }
}

struct GlobExecutor;

#[async_trait::async_trait]
impl super::ToolExecuteWithContext for GlobExecutor {
    fn execute<'a>(
        &'a self,
        services: &'a ToolServices,
        input: &'a Value,
        context: &'a Context,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ToolOutcome, ToolFailure>> + Send + 'a>,
    > {
        Box::pin(async move { run(services, input, context).await })
    }
}

/// `toModelContent` (glob.ts).
#[must_use]
pub fn to_model_content(entries: &[String], truncated: bool) -> String {
    let mut lines: Vec<String> =
        if entries.is_empty() { vec!["No files found".to_string()] } else { entries.to_vec() };
    if truncated {
        lines.push(String::new());
        lines.push(format!(
            "(Results are truncated: showing first {} results. Consider using a more specific path or pattern.)",
            entries.len()
        ));
    }
    lines.join("\n")
}

async fn run(
    services: &ToolServices,
    input: &Value,
    context: &Context,
) -> Result<ToolOutcome, ToolFailure> {
    let pattern = input["pattern"].as_str().unwrap_or_default().to_string();
    // `input.path === "undefined" | "null"` model quirks degrade to the
    // default root.
    let path =
        input["path"].as_str().filter(|p| *p != "undefined" && *p != "null").map(str::to_string);
    let hidden = input["hidden"].as_bool().unwrap_or(false);
    let limit = input["limit"].as_u64().map_or(DEFAULT_SEARCH_LIMIT, |v| v as usize);

    let invocation = Invocation {
        session_id: context.session_id.clone(),
        agent: Some(context.agent.clone()),
        message_id: context.message_id.clone(),
        id: context.call_id.clone(),
    };
    let target = services.access.resolve(&ResolveInput {
        path: path.clone().unwrap_or_else(|| ".".into()),
        kind: Some(Kind::Directory),
    });
    services
        .access
        .authorize_external(std::slice::from_ref(&target), &invocation, None)
        .await
        .map_err(|error| blanket(&pattern, error))?;

    let mut assert_input = crate::permission::AssertInput::new(
        context.session_id.clone(),
        "glob",
        vec![pattern.clone()],
    );
    assert_input.agent = Some(context.agent.clone());
    assert_input.save = Some(vec!["*".to_string()]);
    assert_input.metadata = Some(
        serde_json::from_value(serde_json::json!({
            "root": path.clone().unwrap_or_else(|| ".".into()),
            "path": path,
            "hidden": hidden,
            "limit": input["limit"].as_u64(),
        }))
        .unwrap_or_default(),
    );
    assert_input.source = super::tool_source(context);
    if let Err(error) = services.permission.assert(assert_input).await {
        return Err(blanket(&pattern, error));
    }

    let metadata = std::fs::metadata(&target.absolute).map_err(|_| {
        ToolFailure::message(format!(
            "Search path does not exist: {}",
            path.clone().unwrap_or_else(|| ".".into())
        ))
    })?;
    if !metadata.is_dir() {
        return Err(ToolFailure::message(format!(
            "Search path is not a directory: {}",
            path.clone().unwrap_or_else(|| ".".into())
        )));
    }
    let root = target.absolute.clone();
    let found =
        ripgrep_glob(&root, &pattern, hidden, limit + 1).await.map_err(|error| match error {
            super::grep::RipgrepError::Failed(message)
                if message == super::grep::timeout_message() =>
            {
                ToolFailure::message(message)
            }
            other => ToolFailure {
                message: format!("Unable to find files matching {pattern}"),
                cause: Some(super::FailureCause::Nested(other.to_string())),
            },
        })?;
    let truncated = found.len() > limit;
    let entries: Vec<String> = found.into_iter().take(limit).collect();
    let display: Vec<String> = entries
        .iter()
        .map(|relative| root.join(relative).to_string_lossy().replace('\\', "/"))
        .collect();
    let _ = Path::new(&root);
    Ok(ToolOutcome {
        content: vec![Content::Text { text: to_model_content(&display, truncated) }],
        metadata: Some(
            serde_json::from_value(serde_json::json!({
                "count": entries.len(),
                "truncated": truncated,
            }))
            .unwrap_or_default(),
        ),
        output: None,
    })
}

fn blanket(pattern: &str, error: crate::permission::PermissionError) -> ToolFailure {
    ToolFailure {
        message: format!("Unable to find files matching {pattern}"),
        cause: Some(super::permission_cause(&error)),
    }
}
