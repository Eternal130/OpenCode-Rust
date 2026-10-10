//! Port of `tool.ts` + `tool/runtime.ts` — registration validation,
//! effective names, and the canonical execute pipeline:
//!
//! `tool.execute.before` hook → decode (exact issue format) → leaf execute
//! → foreign-error coercion → content normalization →
//! `tool.execute.after` hook.
//!
//! The wholly-denied snapshot filter lives in
//! [`crate::location::ToolRegistryState::snapshot`] (Wave 0); this module
//! owns the wrapping side that turns a [`super::BuiltinTool`] into a
//! registered executor.

use std::sync::Arc;

use oc_schema::tool::Content;

use super::{BuiltinTool, ToolServices};
use crate::hooks::{
    HookError, ToolExecuteAfterEvent, ToolExecuteBeforeEvent, ToolOutcome as HookToolOutcome,
};
use crate::location::{
    ToolCallContext, ToolExecute, ToolRegistration, ToolRegistryState, ToolRunFailure,
    ToolRunResult,
};

/// `normalizedName` — non `[A-Za-z0-9_-]` characters become `_`.
#[must_use]
pub fn normalized_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect()
}

/// `effectiveName` — `<namespace>_<name>` with dots flattened.
#[must_use]
pub fn effective_name(namespace: Option<&str>, name: &str) -> String {
    match namespace {
        Some(namespace) => format!("{}_{}", namespace.replace('.', "_"), normalized_name(name)),
        None => normalized_name(name),
    }
}

/// Name validity (`^[A-Za-z0-9_-]{1,128}$`).
fn valid_name(name: &str) -> bool {
    (1..=128).contains(&name.len())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Namespace validity (`dot-separated `[A-Za-z0-9_-]{1,64}` segments).
#[must_use]
pub fn valid_namespace(name: &str) -> bool {
    !name.is_empty()
        && name.split('.').all(|segment| {
            (1..=64).contains(&segment.len())
                && segment.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        })
}

/// `registrationError` — `None` when the tool registers.
#[must_use]
pub fn registration_error(tool: &BuiltinTool) -> Option<String> {
    let id = effective_name(None, tool.name);
    if !valid_name(&normalized_name(tool.name)) {
        return Some(format!("Invalid tool name: {}", normalized_name(tool.name)));
    }
    if !tool.codemode && id == "execute" {
        return Some("Tool name \"execute\" is reserved for CodeMode".to_string());
    }
    None
}

// ---------------------------------------------------------------------------
// Input decoding (runtime.ts decodeInput + formatInputIssues)
// ---------------------------------------------------------------------------

/// One validation issue (path + message).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub path: String,
    pub message: String,
}

/// A minimal JSON-Schema validator over the keyword subset the built-in
/// tool schemas use: object `required`, per-property `type`, `enum`,
/// `minLength`/`maxLength`, `minimum`/`maximum`, and nested object/array
/// items. Unknown keywords are ignored (like effect's conversion passthrough).
#[must_use]
pub fn validate_input(schema: &serde_json::Value, value: &serde_json::Value) -> Vec<Issue> {
    let mut issues = Vec::new();
    validate(schema, value, "", &mut issues);
    issues
}

fn type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

fn join_path(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_string()
    } else {
        format!("{path}.{key}")
    }
}

fn custom_message(schema: &serde_json::Value) -> Option<&str> {
    schema.get("x-opencode-message").and_then(|v| v.as_str())
}

fn validate(
    schema: &serde_json::Value,
    value: &serde_json::Value,
    path: &str,
    issues: &mut Vec<Issue>,
) {
    let Some(object) = schema.as_object() else { return };
    if let Some(expected) = object.get("type").and_then(|v| v.as_str()) {
        let matches = match (expected, value) {
            ("string", serde_json::Value::String(_)) => true,
            ("number" | "integer", serde_json::Value::Number(n)) => {
                expected == "number" || n.is_i64() || n.is_u64()
            }
            ("boolean", serde_json::Value::Bool(_)) => true,
            ("object", serde_json::Value::Object(_)) => true,
            ("array", serde_json::Value::Array(_)) => true,
            ("null", serde_json::Value::Null) => true,
            _ => false,
        };
        if !matches {
            let message = custom_message(schema)
                .map(str::to_string)
                .unwrap_or_else(|| format!("Expected {expected}, received {}", type_name(value)));
            issues.push(Issue {
                path: if path.is_empty() { "root".to_string() } else { path.to_string() },
                message,
            });
            return;
        }
    }
    if let Some(min_length) = object.get("minLength").and_then(|v| v.as_u64()) {
        if let serde_json::Value::String(text) = value {
            if (text.chars().count() as u64) < min_length {
                let message = custom_message(schema)
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("must NOT have fewer than {min_length} characters"));
                issues.push(Issue {
                    path: if path.is_empty() { "root".to_string() } else { path.to_string() },
                    message,
                });
            }
        }
    }
    if let Some(enum_values) = object.get("enum").and_then(|v| v.as_array()) {
        if !enum_values.contains(value) {
            issues.push(Issue {
                path: if path.is_empty() { "root".to_string() } else { path.to_string() },
                message: format!(
                    "must be equal to one of the allowed values: {}",
                    serde_json::to_string(enum_values).unwrap_or_default()
                ),
            });
        }
    }
    match value {
        serde_json::Value::Object(fields) => {
            if let (Some(required), Some(properties)) =
                (object.get("required").and_then(|v| v.as_array()), object.get("properties"))
            {
                for key in required {
                    if let Some(key) = key.as_str() {
                        if !fields.contains_key(key) {
                            issues.push(Issue {
                                path: if path.is_empty() {
                                    key.to_string()
                                } else {
                                    format!("{path}.{key}")
                                },
                                message: "is required".to_string(),
                            });
                        }
                    }
                }
                for (key, field) in fields {
                    if let Some(field_schema) =
                        properties.get(key).or_else(|| object.get("additionalProperties"))
                    {
                        validate(field_schema, field, &join_path(path, key), issues);
                    }
                }
            }
        }
        serde_json::Value::Array(items) => {
            if let Some(item_schema) = object.get("items") {
                for (index, item) in items.iter().enumerate() {
                    validate(item_schema, item, &format!("{path}[{index}]"), issues);
                }
            }
        }
        _ => {}
    }
}

/// `formatInputIssues` — the exact decode-failure message.
#[must_use]
pub fn format_input_issues(tool: &str, issues: &[Issue], value: &serde_json::Value) -> String {
    let mut details: Vec<String> =
        issues.iter().take(5).map(|issue| format!("- {}: {}", issue.path, issue.message)).collect();
    if issues.len() > 5 {
        details.push(format!(
            "- ...and {} more {}",
            issues.len() - 5,
            if issues.len() == 6 { "issue" } else { "issues" }
        ));
    }
    format!(
        "Invalid arguments for tool \"{tool}\":\n{}\n\nArguments provided:\n{}\n\nUpdate the arguments and call the tool again.",
        details.join("\n"),
        serde_json::to_string_pretty(value).unwrap_or_default()
    )
}

// ---------------------------------------------------------------------------
// Content normalization (runtime.ts normalizeContent)
// ---------------------------------------------------------------------------

/// `normalizeContent(value, output)` — string → one text part; non-empty
/// content kept; else the stringified output.
#[must_use]
pub fn normalize_content(content: ToolContent, output: Option<&serde_json::Value>) -> Vec<Content> {
    match content {
        ToolContent::Text(text) => vec![Content::Text { text }],
        ToolContent::Parts(parts) if !parts.is_empty() => parts,
        ToolContent::Parts(_) => vec![Content::Text { text: stringify(output) }],
    }
}

/// `stringify`.
#[must_use]
pub fn stringify(value: Option<&serde_json::Value>) -> String {
    match value {
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(other) => serde_json::to_string(other).unwrap_or_else(|_| other.to_string()),
        None => "undefined".to_string(),
    }
}

/// The leaf's content surface: a plain string or explicit parts.
pub enum ToolContent {
    Text(String),
    Parts(Vec<Content>),
}

// ---------------------------------------------------------------------------
// Registration (the wrapped executor)
// ---------------------------------------------------------------------------

/// Wraps one built-in leaf into the pipeline and registers it.
pub fn register(
    state: &ToolRegistryState,
    hooks: &Arc<crate::hooks::HookRegistry>,
    services: &Arc<ToolServices>,
    tool: BuiltinTool,
) -> Result<(), String> {
    if let Some(error) = registration_error(&tool) {
        return Err(error);
    }
    let mut registration =
        ToolRegistration::new(tool.name, tool.description.clone(), tool.input.clone());
    registration.namespace = None;
    registration.permission = tool.permission.map(str::to_string);
    registration.codemode = tool.codemode;
    state.register(registration);

    let pipeline = PipelineExecutor {
        name: effective_name(None, tool.name),
        schema: tool.input,
        leaf: tool.execute,
        services: Arc::clone(services),
        hooks: Arc::clone(hooks),
    };
    state.register_executor(tool.name, Arc::new(pipeline));
    Ok(())
}

/// The pipeline executor around an arbitrary leaf (the
/// [`super::codemode`] surface: the synthesized `execute` tool skips
/// [`registration_error`]'s reserved-name rule the way v2's snapshot
/// synthesizes it outside the editor path).
pub(crate) fn pipeline_executor(
    name: &str,
    schema: serde_json::Value,
    leaf: Arc<dyn super::ToolExecuteWithContext>,
    services: &Arc<ToolServices>,
    hooks: &Arc<crate::hooks::HookRegistry>,
) -> Arc<dyn ToolExecute> {
    Arc::new(PipelineExecutor {
        name: name.to_string(),
        schema,
        leaf,
        services: Arc::clone(services),
        hooks: Arc::clone(hooks),
    })
}

/// Whether the leaf failure is a user decline (tunnels as an interrupt).
fn is_declined(failure: &super::ToolFailure) -> bool {
    matches!(&failure.cause, Some(super::FailureCause::Declined { .. }))
}

/// The pipeline executor (runtime.ts `execute` + tool.ts `executeTool`).
struct PipelineExecutor {
    name: String,
    schema: serde_json::Value,
    leaf: Arc<dyn super::ToolExecuteWithContext>,
    services: Arc<ToolServices>,
    hooks: Arc<crate::hooks::HookRegistry>,
}

impl PipelineExecutor {
    async fn run(
        &self,
        input: &serde_json::Value,
        context: &ToolCallContext,
    ) -> Result<ToolRunResult, ToolRunFailure> {
        // tool.execute.before (reject/mutate).
        let before = ToolExecuteBeforeEvent {
            tool: self.name.clone(),
            session_id: context.session_id.clone(),
            agent: context.agent.clone(),
            message_id: oc_schema::id::MessageId::parse(context.message_id.clone())
                .unwrap_or_else(|error| oc_schema::id::MessageId(error.value)),
            call_id: context.call_id.clone(),
            input: input.clone(),
        };
        let before = match self.hooks.dispatch_tool_execute_before(before).await {
            Ok(event) => event,
            Err(HookError::ToolRejected(error)) => {
                let message = error.message.clone();
                return Err(ToolRunFailure::Error { message, metadata: error.metadata });
            }
            Err(error) => {
                return Err(ToolRunFailure::Error { message: error.to_string(), metadata: None });
            }
        };
        let input = before.input;

        // decodeInput — schema issues become the exact Tool.Error message.
        let issues = validate_input(&self.schema, &input);
        if !issues.is_empty() {
            return Err(ToolRunFailure::Error {
                message: format_input_issues(&self.name, &issues, &input),
                metadata: None,
            });
        }

        // Leaf execute — foreign errors coerce at the untrusted boundary.
        let outcome = (self.leaf.execute(&self.services, &input, context)).await;
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(failure) => {
                if is_declined(&failure) {
                    // Declines tunnel: the step interrupts, never tool output.
                    return Err(ToolRunFailure::Declined { message: failure.message });
                }
                let message = failure.visible_message();
                let failure = match &failure.cause {
                    // `toSessionError` unwraps Blocked/Corrected to
                    // `permission.rejected` with the engine's message.
                    Some(super::FailureCause::Permission { message }) => {
                        ToolRunFailure::PermissionRejected {
                            message: message.clone(),
                            metadata: None,
                        }
                    }
                    _ => ToolRunFailure::Error { message, metadata: None },
                };
                // v2 fires `execute.after` on the error leg and returns
                // the (possibly plugin-rewritten) error object
                // (`return yield* afterEvent.error`, tool.ts:140): a
                // plugin rewriting the message/metadata shapes the
                // model-visible failure. Apply the mutation; keep the
                // variant classification (Error vs PermissionRejected)
                // ours carries.
                let shaped = self
                    .emit_after(
                        &input,
                        context,
                        HookToolOutcome::Error {
                            error: oc_schema::tool::ToolError {
                                message: failure.message().to_string(),
                                error: None,
                                metadata: None,
                            },
                        },
                    )
                    .await;
                return Err(match failure {
                    ToolRunFailure::Error { metadata, .. } => ToolRunFailure::Error {
                        message: shaped.message,
                        metadata: shaped.metadata.or(metadata),
                    },
                    ToolRunFailure::PermissionRejected { metadata, .. } => {
                        ToolRunFailure::PermissionRejected {
                            message: shaped.message,
                            metadata: shaped.metadata.or(metadata),
                        }
                    }
                    other => other,
                });
            }
        };
        // tool.execute.after (result mutation).
        let after = ToolExecuteAfterEvent {
            tool: self.name.clone(),
            session_id: context.session_id.clone(),
            agent: context.agent.clone(),
            message_id: oc_schema::id::MessageId::parse(context.message_id.clone())
                .unwrap_or_else(|error| oc_schema::id::MessageId(error.value)),
            call_id: context.call_id.clone(),
            input: input.clone(),
            outcome: HookToolOutcome::Completed {
                result: serde_json::json!({
                    "content": serde_json::to_value(&outcome.content).unwrap_or_default(),
                    "metadata": outcome.metadata.clone().map(|m| serde_json::to_value(&m).unwrap_or_default()).unwrap_or(serde_json::Value::Null),
                }),
            },
        };
        let after = match self.hooks.dispatch_tool_execute_after(after).await {
            Ok(event) => event,
            Err(error) => {
                return Err(ToolRunFailure::Error { message: error.to_string(), metadata: None });
            }
        };
        // The hook may rewrite content/metadata through the JSON shape.
        let mut content = outcome.content;
        let mut metadata = outcome.metadata;
        if let HookToolOutcome::Completed { result } = &after.outcome {
            // v2's normalizeContent applies an emptied array too (an
            // empty content falls back to the stringified `output`,
            // tool/runtime.ts:259) — a plugin suppressing output must
            // suppress it here as well, not keep the original.
            if let Ok(parsed) = serde_json::from_value::<Vec<Content>>(result["content"].clone()) {
                if !parsed.is_empty() {
                    content = parsed;
                } else if let Some(output) = result.get("output") {
                    let text = match output {
                        serde_json::Value::String(text) => text.clone(),
                        other => other.to_string(),
                    };
                    content = vec![Content::Text { text }];
                } else {
                    content = parsed;
                }
            }
            if let Some(parsed) = result["metadata"].as_object() {
                metadata = Some(parsed.clone().into_iter().collect());
            }
        }
        Ok(ToolRunResult { content, metadata })
    }

    /// Fires `tool.execute.after` for the error leg and returns the
    /// SHAPED error (v2 `return yield* afterEvent.error` — plugin
    /// rewrites apply). A dispatch failure returns the original
    /// message: the failure channel is already taken.
    async fn emit_after(
        &self,
        input: &serde_json::Value,
        context: &ToolCallContext,
        outcome: HookToolOutcome,
    ) -> oc_schema::tool::ToolError {
        let fallback = match &outcome {
            HookToolOutcome::Error { error } => error.clone(),
            HookToolOutcome::Completed { .. } => oc_schema::tool::ToolError {
                message: String::new(),
                error: None,
                metadata: None,
            },
        };
        let event = ToolExecuteAfterEvent {
            tool: self.name.clone(),
            session_id: context.session_id.clone(),
            agent: context.agent.clone(),
            message_id: oc_schema::id::MessageId::parse(context.message_id.clone())
                .unwrap_or_else(|error| oc_schema::id::MessageId(error.value)),
            call_id: context.call_id.clone(),
            input: input.clone(),
            outcome,
        };
        match self.hooks.dispatch_tool_execute_after(event).await {
            Ok(shaped) => match shaped.outcome {
                HookToolOutcome::Error { error } => error,
                HookToolOutcome::Completed { .. } => fallback,
            },
            Err(_) => fallback,
        }
    }
}

#[async_trait::async_trait]
impl ToolExecute for PipelineExecutor {
    async fn execute(&self, input: &serde_json::Value) -> Result<ToolRunResult, ToolRunFailure> {
        let context = ToolCallContext {
            session_id: oc_schema::id::SessionId("ses_tool_pipeline".into()),
            agent: oc_schema::id::AgentId::parse("build"),
            message_id: String::new(),
            call_id: String::new(),
        };
        self.execute_with_context(input, &context).await
    }

    async fn execute_with_context(
        &self,
        input: &serde_json::Value,
        context: &ToolCallContext,
    ) -> Result<ToolRunResult, ToolRunFailure> {
        self.run(input, context).await
    }
}
