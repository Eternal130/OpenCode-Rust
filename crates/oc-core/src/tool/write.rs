//! Port of `tool/plugin/write.ts` — the file-write leaf with the
//! added/modified diff preview and BOM-preserving write (core-runner-map
//! §4.7).

use oc_schema::session_support::FileDiffStatus;
use oc_schema::tool::Content;
use serde_json::Value;

use super::file_diff::file_diff;
use super::{BuiltinTool, Context, ToolFailure, ToolOutcome, ToolServices};
use crate::file_access::{Invocation, Kind, ResolveInput};
use crate::file_mutation::{bom_split, MutationTarget};

pub fn description() -> String {
    "Writes a file to the local filesystem, overwriting if one exists.\n\nMissing parent directories are created automatically.\n\nUse this tool to create new files or overwrite existing files. For partial changes, use the edit tool instead.".to_string()
}

pub fn schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "Path to the file to write to" },
            "content": { "type": "string", "description": "Content to write to the file" }
        },
        "required": ["path", "content"]
    })
}

pub fn tool(_services: &std::sync::Arc<ToolServices>) -> BuiltinTool {
    BuiltinTool {
        name: "write",
        description: description(),
        input: schema(),
        permission: Some("edit"),
        codemode: false,
        execute: std::sync::Arc::new(WriteExecutor),
    }
}

struct WriteExecutor;

#[async_trait::async_trait]
impl super::ToolExecuteWithContext for WriteExecutor {
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

async fn run(
    services: &ToolServices,
    input: &Value,
    context: &Context,
) -> Result<ToolOutcome, ToolFailure> {
    let path = input["path"].as_str().unwrap_or_default().to_string();
    let content = input["content"].as_str().unwrap_or_default().to_string();

    let target =
        services.access.resolve(&ResolveInput { path: path.clone(), kind: Some(Kind::File) });
    let invocation = Invocation {
        session_id: context.session_id.clone(),
        agent: Some(context.agent.clone()),
        message_id: context.message_id.clone(),
        id: context.call_id.clone(),
    };
    services
        .access
        .authorize_external(std::slice::from_ref(&target), &invocation, None)
        .await
        .map_err(|error| blanket(&path, error))?;

    let current = crate::file_mutation::read_text(&target.absolute).ok();
    let (_, next_text) = bom_split(&content);
    let preview = file_diff(
        &target.resource,
        current.as_ref().map_or("", |c| c.text.as_str()),
        next_text,
        if current.is_some() { FileDiffStatus::Modified } else { FileDiffStatus::Added },
    );
    let mut assert_input = crate::permission::AssertInput::new(
        context.session_id.clone(),
        "edit",
        vec![target.resource.clone()],
    );
    assert_input.agent = Some(context.agent.clone());
    assert_input.save = Some(vec!["*".to_string()]);
    assert_input.metadata = Some(
        serde_json::to_value(serde_json::json!({ "files": [preview] }))
            .ok()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default(),
    );
    assert_input.source = super::tool_source(context);
    if let Err(error) = services.permission.assert(assert_input).await {
        return Err(blanket(&path, error));
    }

    let mutation_target =
        MutationTarget { absolute: target.absolute.clone(), resource: target.resource.clone() };
    let result =
        services.mutation.write_text_preserving_bom(&mutation_target, &content).await.map_err(
            |error| ToolFailure {
                message: format!("Unable to write {path}"),
                cause: Some(super::FailureCause::Nested(error.to_string())),
            },
        )?;
    let bom = crate::file_mutation::read_text(&target.absolute)
        .map(|content| content.bom)
        .unwrap_or(false);
    if services.formatter.file(&target.absolute) {
        let _ = crate::file_mutation::sync_text_bom(&target.absolute, bom);
    }
    Ok(ToolOutcome {
        content: vec![Content::Text {
            text: format!(
                "{} file successfully: {}",
                if result.existed { "Wrote" } else { "Created" },
                result.resource
            ),
        }],
        metadata: None,
        output: Some(serde_json::json!({
            "operation": "write",
            "target": result.target.to_string_lossy().replace('\\', "/"),
            "resource": result.resource,
            "existed": result.existed,
        })),
    })
}

fn blanket(path: &str, error: crate::permission::PermissionError) -> ToolFailure {
    ToolFailure {
        message: format!("Unable to write {path}"),
        cause: Some(super::permission_cause(&error)),
    }
}
