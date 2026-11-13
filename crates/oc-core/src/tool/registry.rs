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
    namespaced_registration_error(tool.namespace, tool.name, tool.codemode)
}

/// G-360③ — the namespaced face of [`registration_error`] (the MCP
/// bridge's registration gate): the ref's editor.add rules (tool.ts
/// 304-320) applied to a `(namespace, name, codemode)` triple —
/// namespace segments `^[A-Za-z0-9_-]{1,64}$` when present, the
/// normalized tool name `^[A-Za-z0-9_-]{1,128}$`, and the Code Mode
/// `execute` reservation on the effective id.
#[must_use]
pub fn namespaced_registration_error(
    namespace: Option<&str>,
    name: &str,
    codemode: bool,
) -> Option<String> {
    if let Some(namespace) = namespace {
        if !valid_namespace(namespace) {
            return Some(format!("Invalid tool namespace: {namespace:?}"));
        }
    }
    let normalized = normalized_name(name);
    if !valid_name(&normalized) {
        return Some(format!("Invalid tool name: {normalized}"));
    }
    let id = effective_name(namespace, name);
    if !codemode && id == "execute" {
        return Some("Tool name \"execute\" is reserved for CodeMode".to_string());
    }
    None
}

// ---------------------------------------------------------------------------
// Input decoding (runtime.ts decodeInput + formatInputIssues)
// ---------------------------------------------------------------------------

// G-100: the JSON-Schema validator moved to `oc_ai::json_schema` (the
// shared engine `generateObject` output validation also uses);
// re-exported here so the tool decode pipeline keeps its API.
pub use oc_ai::json_schema::{validate_input, Issue};

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
    // runtime.ts:24-25 — a declared output rides the model-facing
    // definition (`outputSchema`). A `Null` schema means undeclared
    // (test/inline leaves that never return an output).
    let output_schema = if tool.output.is_null() { None } else { Some(tool.output.clone()) };
    registration.output.clone_from(&output_schema);
    // `options.namespace` survives registration (v2 editor.add keys the
    // table by effectiveName and keeps options.namespace — opencode.ts's
    // three tools advertise `opencode_*` and the Code Mode catalog lists
    // `opencode.*` paths).
    registration.namespace = tool.namespace.map(str::to_string);
    registration.permission = tool.permission.map(str::to_string);
    registration.codemode = tool.codemode;
    state.register(registration);

    let effective = effective_name(tool.namespace, tool.name);
    // G-230 — every builtin DECLARES its output schema (the ref
    // plugin-tool Output declarations, edit.ts:127 / glob.ts:61 / …):
    // the declared schema enters `encodeOutput` via the with-output
    // constructor (runtime.ts:110-132). A `Null` schema stays the
    // undeclared accept-all scope (test/inline leaves).
    let pipeline = pipeline_executor_with_output(
        &effective,
        tool.input,
        output_schema,
        tool.execute,
        services,
        hooks,
    );
    // G-291 — the executor table is keyed by the SAME effective name
    // as the registration table (tool.ts:188 `editor.tools.set(id, …)`).
    state.register_executor(effective, pipeline);
    Ok(())
}

/// The pipeline executor around an arbitrary leaf with NO declared
/// output (the undeclared scope: no `output` key passes through, an
/// `output` key DIES — runtime.ts:45-46). Used by the g230 tests.
#[cfg(test)]
pub(crate) fn pipeline_executor(
    name: &str,
    schema: serde_json::Value,
    leaf: Arc<dyn super::ToolExecuteWithContext>,
    services: &Arc<ToolServices>,
    hooks: &Arc<crate::hooks::HookRegistry>,
) -> Arc<dyn ToolExecute> {
    pipeline_executor_with_output(name, schema, None, leaf, services, hooks)
}

/// G-230 — the pipeline executor with a DECLARED output schema: the
/// returned output is validated against it (runtime.ts:110-132
/// `encodeOutput`); see [`PipelineExecutor::output_schema`] for the
/// undeclared fallback scope.
pub(crate) fn pipeline_executor_with_output(
    name: &str,
    schema: serde_json::Value,
    output_schema: Option<serde_json::Value>,
    leaf: Arc<dyn super::ToolExecuteWithContext>,
    services: &Arc<ToolServices>,
    hooks: &Arc<crate::hooks::HookRegistry>,
) -> Arc<dyn ToolExecute> {
    Arc::new(PipelineExecutor {
        name: name.to_string(),
        schema,
        output_schema,
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
    /// G-230 — the tool's DECLARED output schema (v2 `output`,
    /// runtime.ts:110 `encodeOutput`). `None` is the undeclared scope:
    /// runtime.ts:45-46 DIES when the tool still returns an `output`
    /// key. Every BUILTIN declares its output (the ref plugin-tool
    /// Output declarations, ported as `BuiltinTool::output`); the MCP
    /// bridge mirrors mcp.ts:48 `output: tool.outputSchema ?? {}` —
    /// a server-undeclared tool is declared against the accept-all
    /// `{}`, never `None` (G-242).
    output_schema: Option<serde_json::Value>,
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
        // The after hook fires for ALL Tool.Error outcomes, including
        // decode failure (tool.ts:130-137: decodeInput is caught inside
        // `execute`, so `yield* afterEvent.error` runs before the
        // return).
        let issues = validate_input(&self.schema, &input);
        if !issues.is_empty() {
            let shaped = self
                .emit_after(
                    &input,
                    context,
                    HookToolOutcome::Error {
                        error: oc_schema::tool::ToolError {
                            message: format_input_issues(&self.name, &issues, &input),
                            error: None,
                            metadata: None,
                        },
                    },
                )
                .await;
            return Err(ToolRunFailure::Error {
                message: shaped.message,
                metadata: shaped.metadata,
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
        // G-230 — `encodeOutput` (tool/runtime.ts:45-56, :110-132),
        // applied to the LEAF's output before `tool.execute.after`
        // (the after event carries the validated output, tool.ts:142;
        // a hook rewriting it afterwards is the trusted plugin
        // surface, exactly like ref — no re-validation).
        //
        // * DECLARED + present → validate; issues fail the call with
        //   ref's message shape (runtime.ts:116's issue-joining
        //   wording from the standard-schema branch, :119).
        // * DECLARED + missing → `Tool did not return its declared
        //   output` (runtime.ts:53).
        // * UNDECLARED → runtime.ts:46's die rule: an undeclared tool
        //   that returns an `output` key is a contract violation and
        //   fails the call (ref dies; this port surfaces it as the
        //   standard ToolRunFailure::Error shape — Rust has no defect
        //   variant, a bounded deviation).
        // * UNDECLARED + no output → the mcp.ts:48 `?? {}` accept-all
        //   fallback (see `output_schema`): pass through unvalidated.
        let outcome = match (&self.output_schema, outcome) {
            (Some(output_schema), mut outcome) => {
                let Some(output) = outcome.output.take() else {
                    return Err(ToolRunFailure::Error {
                        message: "Tool did not return its declared output".to_owned(),
                        metadata: None,
                    });
                };
                let issues = validate_input(output_schema, &output);
                if !issues.is_empty() {
                    let details = issues
                        .iter()
                        .map(|issue| issue.message.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Err(ToolRunFailure::Error {
                        message: format!(
                            "Tool returned an invalid value for its output schema: {details}"
                        ),
                        metadata: None,
                    });
                }
                outcome.output = Some(output);
                outcome
            }
            // UNDECLARED → runtime.ts:45-46: `"output" in result` on an
            // undeclared tool DIES in ref ("Tool result declared output
            // without an output schema"); surfaced here as the standard
            // ToolRunFailure::Error shape (Rust has no defect variant —
            // bounded deviation). No output key keeps the mcp.ts:48
            // `?? {}` accept-all pass-through.
            (None, outcome) => match outcome.output {
                Some(_) => Err(ToolRunFailure::Error {
                    message: "Tool result declared output without an output schema".to_owned(),
                    metadata: None,
                }),
                None => Ok(outcome),
            }?,
        };
        // tool.execute.after (result mutation). The event carries the
        // declared output when present (tool.ts:142-146:
        // `...(output === undefined ? {} : { output })`).
        let mut result_json = serde_json::Map::new();
        if let Some(output) = &outcome.output {
            result_json.insert("output".to_string(), output.clone());
        }
        result_json.insert(
            "content".to_string(),
            serde_json::to_value(&outcome.content).unwrap_or_default(),
        );
        result_json.insert(
            "metadata".to_string(),
            outcome
                .metadata
                .clone()
                .map(|m| serde_json::to_value(&m).unwrap_or_default())
                .unwrap_or(serde_json::Value::Null),
        );
        let after = ToolExecuteAfterEvent {
            tool: self.name.clone(),
            session_id: context.session_id.clone(),
            agent: context.agent.clone(),
            message_id: oc_schema::id::MessageId::parse(context.message_id.clone())
                .unwrap_or_else(|error| oc_schema::id::MessageId(error.value)),
            call_id: context.call_id.clone(),
            input: input.clone(),
            outcome: HookToolOutcome::Completed { result: result_json.into() },
        };
        let after = match self.hooks.dispatch_tool_execute_after(after).await {
            Ok(event) => event,
            Err(error) => {
                return Err(ToolRunFailure::Error { message: error.to_string(), metadata: None });
            }
        };
        // The hook may rewrite output/content/metadata through the JSON
        // shape (the returned `output` is the post-hook value, tool.ts:153).
        let mut content = outcome.content;
        let mut metadata = outcome.metadata;
        let mut output = outcome.output;
        if let HookToolOutcome::Completed { result } = &after.outcome {
            // v2's normalizeContent applies an emptied array too (an
            // empty content falls back to the stringified `output`,
            // tool/runtime.ts:259) — a plugin suppressing output must
            // suppress it here as well, not keep the original.
            if let Some(hook_output) = result.get("output") {
                output = Some(hook_output.clone());
            } else {
                output = None;
            }
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
        Ok(ToolRunResult { content, metadata, output })
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
            HookToolOutcome::Completed { .. } => {
                oc_schema::tool::ToolError { message: String::new(), error: None, metadata: None }
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn validate_input_enforces_numeric_bounds() {
        // v2 decodes tool inputs through effect Schemas with bound checks
        // (e.g. webfetch `Timeout` is >0 and <=120, webfetch.ts:26) — an
        // out-of-range value is an argument error, never silently clamped.
        let schema = json!({
            "type": "object",
            "properties": {
                "timeout": { "type": "integer", "minimum": 1, "maximum": 120 }
            },
            "required": ["timeout"]
        });
        assert!(validate_input(&schema, &json!({ "timeout": 30 })).is_empty());
        let issues = validate_input(&schema, &json!({ "timeout": 500 }));
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].message, "must be less than or equal to 120");
        let issues = validate_input(&schema, &json!({ "timeout": 0 }));
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].message, "must be greater than or equal to 1");
    }

    // G-59 — the fuller keyword subset (MCP external schemas flow through
    // the same validator; each new keyword needs a pin).

    #[test]
    fn validate_input_enforces_pattern() {
        let schema = json!({
            "type": "object",
            "properties": { "name": { "type": "string", "pattern": "^[a-z]+$" } },
            "required": ["name"]
        });
        assert!(validate_input(&schema, &json!({ "name": "abc" })).is_empty());
        let issues = validate_input(&schema, &json!({ "name": "ABC" }));
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].path, "name");
        // Invalid regex stays lenient (no validator crash).
        let lenient = json!({ "type": "object", "properties": {
            "x": { "type": "string", "pattern": "[" }
        }});
        assert!(validate_input(&lenient, &json!({ "x": "any" })).is_empty());
    }

    #[test]
    fn validate_input_enforces_string_lengths() {
        let schema = json!({
            "type": "object",
            "properties": { "s": { "type": "string", "minLength": 2, "maxLength": 4 } }
        });
        assert!(validate_input(&schema, &json!({ "s": "abc" })).is_empty());
        let issues = validate_input(&schema, &json!({ "s": "a" }));
        assert_eq!(issues[0].message, "must NOT have fewer than 2 characters");
        let issues = validate_input(&schema, &json!({ "s": "abcde" }));
        assert_eq!(issues[0].message, "must NOT have more than 4 characters");
    }

    #[test]
    fn validate_input_enforces_one_of() {
        let schema = json!({
            "oneOf": [
                { "type": "object", "properties": { "a": { "type": "string" } }, "required": ["a"],
                  "additionalProperties": false },
                { "type": "object", "properties": { "b": { "type": "integer" } }, "required": ["b"],
                  "additionalProperties": false }
            ]
        });
        assert!(validate_input(&schema, &json!({ "a": "x" })).is_empty());
        assert!(validate_input(&schema, &json!({ "b": 1 })).is_empty());
        // Matches both → oneOf failure. (Strict branches make mixed shapes
        // match neither; loosen to observe the >1 case.)
        let overlap = json!({
            "oneOf": [
                { "type": "object", "properties": { "a": { "type": "string" } } },
                { "type": "object", "properties": { "b": { "type": "integer" } } }
            ]
        });
        let issues = validate_input(&overlap, &json!({ "a": "x", "b": 1 }));
        assert_eq!(issues[0].message, "must match exactly one schema in oneOf");
        let issues = validate_input(&schema, &json!({ "a": "x", "b": 1 }));
        assert_eq!(issues[0].message, "must match exactly one schema in oneOf");
        // Matches neither.
        let issues = validate_input(&schema, &json!({ "c": true }));
        assert!(issues
            .iter()
            .any(|issue| issue.message == "must match exactly one schema in oneOf"));
    }

    #[test]
    fn validate_input_enforces_any_of_and_all_of() {
        let any = json!({
            "anyOf": [ { "type": "string" }, { "type": "integer" } ]
        });
        assert!(validate_input(&any, &json!("x")).is_empty());
        let issues = validate_input(&any, &json!(true));
        assert_eq!(issues[0].message, "must match a schema in anyOf");

        let all = json!({
            "allOf": [
                { "type": "string", "minLength": 2 },
                { "pattern": "^a" }
            ]
        });
        assert!(validate_input(&all, &json!("ab")).is_empty());
        let issues = validate_input(&all, &json!("bc"));
        assert!(issues.iter().any(|issue| issue.message.contains("pattern")));
    }

    #[test]
    fn validate_input_resolves_local_defs_refs() {
        let schema = json!({
            "$defs": { "hex": { "type": "string", "pattern": "^[0-9a-f]+$" } },
            "type": "object",
            "properties": { "hash": { "$ref": "#/$defs/hex" } },
            "required": ["hash"]
        });
        assert!(validate_input(&schema, &json!({ "hash": "deadbeef" })).is_empty());
        let issues = validate_input(&schema, &json!({ "hash": "XYZ" }));
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].path, "hash");
    }

    #[test]
    fn validate_input_recursive_ref_terminates() {
        let schema = json!({
            "$defs": {
                "node": {
                    "type": "object",
                    "properties": { "child": { "$ref": "#/$defs/node" } }
                }
            },
            "$ref": "#/$defs/node"
        });
        assert!(validate_input(&schema, &json!({ "child": { "child": {} } })).is_empty());
    }

    #[test]
    fn validate_input_enforces_array_bounds_and_unique_items() {
        let schema = json!({
            "type": "object",
            "properties": {
                "xs": {
                    "type": "array",
                    "items": { "type": "integer" },
                    "minItems": 1,
                    "maxItems": 3,
                    "uniqueItems": true
                }
            }
        });
        assert!(validate_input(&schema, &json!({ "xs": [1, 2] })).is_empty());
        let issues = validate_input(&schema, &json!({ "xs": [] }));
        assert_eq!(issues[0].message, "must NOT have fewer than 1 items");
        let issues = validate_input(&schema, &json!({ "xs": [1, 2, 3, 4] }));
        assert_eq!(issues[0].message, "must NOT have more than 3 items");
        let issues = validate_input(&schema, &json!({ "xs": [1, 1] }));
        assert_eq!(issues[0].message, "must NOT have duplicate items");
    }

    /// Review fix1 — `additionalProperties` applies with or without a
    /// `properties` key: a `properties`-less closed object still rejects
    /// extras, and the schema form still validates/rejects them.
    #[test]
    fn validate_input_additional_properties_without_properties() {
        // Bool form, no `properties`: every key is extra.
        let closed = json!({ "type": "object", "additionalProperties": false });
        assert!(validate_input(&closed, &json!({})).is_empty());
        let issues = validate_input(&closed, &json!({ "extra": 1 }));
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].path, "extra");
        assert_eq!(issues[0].message, "must NOT have additional properties");

        // Schema form, no `properties`: extras validate against it.
        let typed = json!({ "type": "object", "additionalProperties": { "type": "string" } });
        assert!(validate_input(&typed, &json!({ "any": "ok" })).is_empty());
        let issues = validate_input(&typed, &json!({ "any": 5 }));
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].path, "any");
        assert_eq!(issues[0].message, "Expected string, received number");
    }

    #[test]
    fn validate_input_enforces_additional_properties() {
        let closed = json!({
            "type": "object",
            "properties": { "known": { "type": "string" } },
            "additionalProperties": false
        });
        assert!(validate_input(&closed, &json!({ "known": "x" })).is_empty());
        let issues = validate_input(&closed, &json!({ "known": "x", "extra": 1 }));
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].path, "extra");
        assert_eq!(issues[0].message, "must NOT have additional properties");

        // Schema form: extras are validated against it.
        let open = json!({
            "type": "object",
            "properties": { "known": { "type": "string" } },
            "additionalProperties": { "type": "boolean" }
        });
        assert!(validate_input(&open, &json!({ "flag": true })).is_empty());
        let issues = validate_input(&open, &json!({ "flag": "yes" }));
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].path, "flag");
    }

    // -----------------------------------------------------------------------
    // G-230 — encodeOutput (tool/runtime.ts:45-56, :110-132)
    // -----------------------------------------------------------------------

    use super::super::ToolOutcome;

    /// A leaf returning a fixed outcome (the fake tool under test).
    struct FixedLeaf {
        outcome: super::super::ToolOutcome,
    }

    impl super::super::ToolExecuteWithContext for FixedLeaf {
        fn execute<'a>(
            &'a self,
            _services: &'a super::super::ToolServices,
            _input: &'a serde_json::Value,
            _context: &'a super::super::ToolCallContext,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<super::super::ToolOutcome, super::super::ToolFailure>,
                    > + Send
                    + 'a,
            >,
        > {
            let outcome = self.outcome.clone();
            Box::pin(std::future::ready(Ok(outcome)))
        }
    }

    #[tokio::test]
    async fn g230_declared_schema_rejects_invalid_output() {
        let schema = json!({
            "type": "object",
            "properties": { "count": { "type": "integer" } },
            "required": ["count"],
            "additionalProperties": false,
        });
        let harness = crate::tool::tests::harness().await;
        let hooks = Arc::new(crate::hooks::HookRegistry::new());
        let executor = pipeline_executor_with_output(
            "g230.bad",
            json!({}),
            Some(schema),
            Arc::new(FixedLeaf {
                outcome: ToolOutcome {
                    output: Some(json!({ "count": "many" })),
                    ..Default::default()
                },
            }),
            &harness.services,
            &hooks,
        );
        let failure = executor.execute(&json!({})).await.unwrap_err();
        let ToolRunFailure::Error { message, .. } = &failure else {
            panic!("invalid output must be a plain error: {failure:?}");
        };
        assert!(
            message.starts_with("Tool returned an invalid value for its output schema: "),
            "ref wording (runtime.ts:116): {message}"
        );
    }

    /// G-230 review fix — the shared validator's `const` arm now bites
    /// at the pipeline level: a builtin-style declared schema with a
    /// `const` violation fails the call (previously `const` was an
    /// unknown keyword and validated vacuously).
    #[tokio::test]
    async fn g230_declared_schema_rejects_const_violation() {
        // write.rs-style declaration: `operation` is const "write".
        let schema = json!({
            "type": "object",
            "properties": {
                "operation": { "const": "write" },
                "target": { "type": "string" },
            },
            "required": ["operation", "target"],
            "additionalProperties": false,
        });
        let harness = crate::tool::tests::harness().await;
        let hooks = Arc::new(crate::hooks::HookRegistry::new());
        let executor = pipeline_executor_with_output(
            "g230.const",
            json!({}),
            Some(schema),
            Arc::new(FixedLeaf {
                outcome: ToolOutcome {
                    output: Some(json!({ "operation": "delete", "target": "/tmp/x" })),
                    ..Default::default()
                },
            }),
            &harness.services,
            &hooks,
        );
        let failure = executor.execute(&json!({})).await.unwrap_err();
        let ToolRunFailure::Error { message, .. } = &failure else {
            panic!("const violation must be a plain error: {failure:?}");
        };
        assert!(
            message.starts_with("Tool returned an invalid value for its output schema: "),
            "ref wording (runtime.ts:116): {message}"
        );
        assert!(message.contains("must be equal to constant"), "{message}");
    }

    #[tokio::test]
    async fn g230_declared_schema_accepts_valid_output() {
        let schema = json!({
            "type": "object",
            "properties": { "count": { "type": "integer" } },
            "required": ["count"],
            "additionalProperties": false,
        });
        let harness = crate::tool::tests::harness().await;
        let hooks = Arc::new(crate::hooks::HookRegistry::new());
        let executor = pipeline_executor_with_output(
            "g230.good",
            json!({}),
            Some(schema),
            Arc::new(FixedLeaf {
                outcome: ToolOutcome { output: Some(json!({ "count": 3 })), ..Default::default() },
            }),
            &harness.services,
            &hooks,
        );
        let result = executor.execute(&json!({})).await.expect("valid output passes");
        assert_eq!(result.output, Some(json!({ "count": 3 })));
    }

    /// runtime.ts:53 — declared schema but the tool returned no output.
    #[tokio::test]
    async fn g230_declared_schema_missing_output_fails() {
        let harness = crate::tool::tests::harness().await;
        let hooks = Arc::new(crate::hooks::HookRegistry::new());
        let executor = pipeline_executor_with_output(
            "g230.missing",
            json!({}),
            Some(json!({ "type": "object" })),
            Arc::new(FixedLeaf { outcome: ToolOutcome::default() }),
            &harness.services,
            &hooks,
        );
        let failure = executor.execute(&json!({})).await.unwrap_err();
        let ToolRunFailure::Error { message, .. } = &failure else {
            panic!("missing output must be a plain error: {failure:?}");
        };
        assert_eq!(message, "Tool did not return its declared output");
    }

    /// The UNDECLARED scope — runtime.ts:45-46's die rule: an
    /// undeclared tool that returns an `output` key fails the call
    /// with ref's EXACT text ("Tool result declared output without an
    /// output schema"); surfaced as the standard ToolRunFailure::Error
    /// shape (Rust has no defect variant — bounded deviation).
    #[tokio::test]
    async fn g230_undeclared_output_passes_through_fallback() {
        let harness = crate::tool::tests::harness().await;
        let hooks = Arc::new(crate::hooks::HookRegistry::new());
        let executor = pipeline_executor(
            "g230.fallback",
            json!({}),
            Arc::new(FixedLeaf {
                outcome: ToolOutcome {
                    output: Some(json!({ "count": "many" })),
                    ..Default::default()
                },
            }),
            &harness.services,
            &hooks,
        );
        let failure = executor.execute(&json!({})).await.unwrap_err();
        let ToolRunFailure::Error { message, .. } = &failure else {
            panic!("undeclared output must be a plain error: {failure:?}");
        };
        assert_eq!(message, "Tool result declared output without an output schema");
    }

    /// The undeclared + NO-output scope keeps the mcp.ts:48 `?? {}`
    /// accept-all pass-through (the MCP fallback face).
    #[tokio::test]
    async fn g230_undeclared_without_output_passes_through() {
        let harness = crate::tool::tests::harness().await;
        let hooks = Arc::new(crate::hooks::HookRegistry::new());
        let executor = pipeline_executor(
            "g230.no-output",
            json!({}),
            Arc::new(FixedLeaf {
                outcome: ToolOutcome {
                    content: vec![oc_schema::tool::Content::Text { text: "ok".into() }],
                    ..Default::default()
                },
            }),
            &harness.services,
            &hooks,
        );
        let result = executor.execute(&json!({})).await.expect("no output passes through");
        assert_eq!(result.output, None);
    }

    /// The builtin regression: an output-returning builtin (glob —
    /// now DECLARED, G-230) still executes end to end and its output
    /// validates against the declared schema.
    #[tokio::test]
    async fn g230_builtin_returning_output_still_executes() {
        let harness = crate::tool::tests::harness().await;
        // Stage a file THROUGH the tools (relative paths resolve in the
        // harness's private directory).
        crate::tool::tests::call(
            &harness.tools,
            "write",
            json!({ "path": "src/lib.rs", "content": "fn main() {}" }),
        )
        .await
        .expect("write executes");
        let result =
            crate::tool::tests::call(&harness.tools, "glob", json!({ "pattern": "**/*.rs" }))
                .await
                .expect("glob executes (declared schema validates)");
        let output = result.output.expect("glob returns declared output");
        assert!(output.to_string().contains("lib.rs"), "glob output: {output}");
    }
}
