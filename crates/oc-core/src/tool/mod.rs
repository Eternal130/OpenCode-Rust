//! The Wave C1 tool lane — the six filesystem tools (`read`, `write`,
//! `edit`, `glob`, `grep`, `patch`) plus the shared execution pipeline.
//!
//! Layout (port of `tool.ts` + `tool/runtime.ts` + `tool/plugin/*`):
//!
//! - [`registry`]: effective names, registration validation, the
//!   decode → execute → normalize pipeline with `tool.execute.before/after`
//!   hooks;
//! - [`read`]/[`write`]/[`edit`]/[`glob`]/[`grep`]/[`patch`]: the built-in
//!   leaves, each exporting its `schema()` and an executor over
//!   [`ToolServices`];
//! - [`file_diff`]: the unified-diff renderer (`file-diff.ts` +
//!   `diff`-package `createTwoFilesPatch`);
//! - [`patch_util`]: the patch-language parser (`util/patch.ts`).
//!
//! Every model-facing string is byte-exact against v2 (core-runner-map
//! §4.5–§4.8); the tests pin them.

//! Lint posture: ports of TS sources keep the mechanical-idiom allows the
//! runner established (numeric casts, exhaustive tables, ownership queries).
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::must_use_candidate,
    clippy::needless_pass_by_value,
    clippy::too_many_lines,
    clippy::match_same_arms,
    clippy::redundant_closure,
    clippy::redundant_closure_for_method_calls,
    clippy::option_if_let_else,
    clippy::if_not_else,
    clippy::items_after_statements,
    clippy::map_unwrap_or,
    clippy::uninlined_format_args,
    clippy::type_complexity,
    clippy::large_enum_variant,
    clippy::struct_field_names,
    clippy::unnested_or_patterns,
    clippy::single_match_else,
    clippy::len_without_is_empty,
    clippy::module_name_repetitions,
    clippy::result_large_err,
    clippy::doc_markdown,
    clippy::format_push_string,
    clippy::trivially_copy_pass_by_ref,
    clippy::significant_drop_tightening,
    clippy::large_futures,
    clippy::option_option,
    clippy::assert_is_empty,
    clippy::match_wildcard_for_single_variants,
    clippy::semicolon_if_nothing_returned,
    clippy::format_collect,
    clippy::default_trait_access,
    clippy::ignored_unit_patterns,
    clippy::duration_suboptimal_units,
    clippy::explicit_iter_loop,
    clippy::manual_let_else,
    clippy::unnecessary_wraps,
    clippy::ref_option,
    clippy::unused_async,
    clippy::needless_borrow,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::min_ident_chars,
    clippy::unreadable_literal,
    clippy::similar_names,
    clippy::unused_async_trait_impl,
    clippy::unused_self,
    clippy::doc_lazy_continuation
)]

pub mod edit;
pub mod file_diff;
pub mod glob;
pub mod grep;
pub mod html_markdown;
pub mod opencode_tools;
pub mod patch;
pub mod patch_util;
pub mod question;
pub mod read;
pub mod registry;
pub mod shell;
pub mod skill;
pub mod webfetch;
pub mod websearch;
pub mod websearch_providers;
pub mod write;

// Wave D additions (MCP + subagent lane): the two plugin-backed tools
// `tool/plugin/{subagent,mcp-resource}.ts`. Registered into the tool
// registry through the same `registry` pipeline above.
pub mod mcp_resource;
pub mod subagent;

#[cfg(test)]
mod tests_subagent;

use std::path::PathBuf;
use std::sync::Arc;

use oc_schema::id::SessionId;

use crate::file_access::FileAccess;
use crate::file_mutation::FileMutation;
use crate::formatter::Formatter;
use crate::hooks::InternalPlugin;
use crate::location::ToolCallContext;
use crate::permission::Permission;

/// `Ripgrep` runner configuration (the binary seam).
pub mod ripgrep_runner {
    use std::path::PathBuf;

    /// Resolve the ripgrep binary: `OPENCODE_RG_PATH` → bundled
    /// `<exe_dir>/rg` → `rg` from `PATH`.
    #[must_use]
    pub fn binary() -> PathBuf {
        if let Some(path) = std::env::var_os("OPENCODE_RG_PATH") {
            let path = PathBuf::from(path);
            if path.is_file() {
                return path;
            }
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                let bundled = dir.join("rg");
                if bundled.is_file() {
                    return bundled;
                }
            }
        }
        PathBuf::from("rg")
    }
}

/// The per-location service bundle the tool leaves execute against
/// (the injected half of the v2 layer graph).
pub struct ToolServices {
    /// `location.directory` (relative paths resolve here).
    pub directory: PathBuf,
    /// `location.project.directory` (the worktree boundary).
    pub project_directory: PathBuf,
    pub home: PathBuf,
    /// File-access boundary (resolve + external-directory approval).
    pub access: FileAccess,
    /// The permission engine.
    pub permission: Permission,
    /// Serialized mutations + BOM helpers.
    pub mutation: Arc<FileMutation>,
    /// The post-write formatter hook (no-op until the formatter lane).
    pub formatter: Arc<dyn Formatter>,
    /// The AGENTS.md injection seam (`SessionInstructions`); `None` skips
    /// the read side effect.
    pub instructions: Option<Arc<dyn InstructionInjection>>,
    /// Database handle for instruction-ledger reads.
    pub db: Option<crate::database::Database>,
    /// Event bus for synthetic instruction publishes.
    pub bus: Option<crate::bus::Bus>,
}

impl std::fmt::Debug for ToolServices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolServices")
            .field("directory", &self.directory)
            .field("project_directory", &self.project_directory)
            .finish_non_exhaustive()
    }
}

/// `SessionInstructions.load` — inject AGENTS.md files as durable synthetic
/// instructions (in-flight claim + metadata ledger dedup).
pub trait InstructionInjection: Send + Sync {
    fn load(&self, session_id: &SessionId, paths: &[PathBuf]);
}

/// One built-in tool leaf: registration metadata + the raw executor.
pub struct BuiltinTool {
    /// Bare tool name.
    pub name: &'static str,
    pub description: String,
    /// JSON-Schema of the input (v2 shape).
    pub input: serde_json::Value,
    /// `options.permission` (defaults to the name).
    pub permission: Option<&'static str>,
    /// All six filesystem tools are direct (`codemode: false`).
    pub codemode: bool,
    /// The leaf executor (context-aware).
    pub execute: Arc<dyn ToolExecuteWithContext>,
}

/// The leaf seam — like [`crate::location::ToolExecute`] but always carrying
/// the invocation context.
pub trait ToolExecuteWithContext: Send + Sync {
    fn execute<'a>(
        &'a self,
        services: &'a ToolServices,
        input: &'a serde_json::Value,
        context: &'a ToolCallContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ToolOutcome, ToolFailure>> + Send + 'a>,
    >;
}

/// `Tool.Result` — content + metadata (+ declared output, JSON only here).
#[derive(Debug, Clone, Default)]
pub struct ToolOutcome {
    pub content: Vec<oc_schema::tool::Content>,
    pub metadata: Option<oc_schema::common::Record>,
    /// The declared machine output, when the leaf has one (edit/write/…).
    pub output: Option<serde_json::Value>,
}

/// `ToolFailure` — the leaf-level failure (message + optional cause for the
/// `toSessionError` unwrapping rules).
#[derive(Debug, Clone)]
pub struct ToolFailure {
    pub message: String,
    /// The nested typed error (`error` field on `ToolFailure`).
    pub cause: Option<FailureCause>,
}

impl ToolFailure {
    pub fn message(message: impl Into<String>) -> Self {
        Self { message: message.into(), cause: None }
    }

    /// The model-visible session error after `toSessionError` unwrapping:
    /// a nested error's message wins over the leaf's curated blanket
    /// ("Unable to edit {path}" and friends), exactly like
    /// `to-session-error.ts:60-66`.
    pub fn visible_message(&self) -> String {
        match &self.cause {
            Some(FailureCause::Permission { message }) => message.clone(),
            Some(FailureCause::Nested(message)) => message.clone(),
            // Declines never surface as model-visible output (they tunnel
            // as interrupts); the blanket message stands for diagnostics.
            Some(FailureCause::Declined { .. }) | None => self.message.clone(),
        }
    }
}

/// The nested error taxonomy (`toSessionError(cause.error)` unwrapping).
#[derive(Debug, Clone)]
pub enum FailureCause {
    /// `Permission.BlockedError` / `CorrectedError` — surfaces as
    /// `permission.rejected` with the engine's message
    /// (`Permission denied: {action}` or the corrective feedback).
    Permission { message: String },
    /// `Permission.DeclinedError` — tunnels as a defect and interrupts the
    /// step (never model-visible output).
    Declined { message: String },
    /// Any other nested error: its own message is canonical
    /// (`Cannot read binary file: …`).
    Nested(String),
}

/// Maps a permission-engine failure into the leaf `FailureCause` taxonomy.
pub fn permission_cause(error: &crate::permission::PermissionError) -> FailureCause {
    match error {
        crate::permission::PermissionError::Declined(_) => {
            FailureCause::Declined { message: "permission declined".to_string() }
        }
        crate::permission::PermissionError::Blocked(blocked) => {
            FailureCause::Permission { message: blocked.to_string() }
        }
        crate::permission::PermissionError::Corrected(corrected) => {
            FailureCause::Permission { message: corrected.feedback.clone() }
        }
        other => FailureCause::Nested(other.to_string()),
    }
}

/// Registers one built-in leaf into a location's tool state + hook registry
/// through the full pipeline (see [`registry::register`]). `pub(crate)` for
/// the supervisor loop below and focused tests.
pub(crate) fn register_builtin(
    state: &crate::location::ToolRegistryState,
    hooks: &Arc<crate::hooks::HookRegistry>,
    services: &Arc<ToolServices>,
    tool: BuiltinTool,
) -> Result<(), String> {
    registry::register(state, hooks, services, tool)
}

/// Registers every built-in filesystem leaf (what [`builtin_plugins`]
/// invokes per location boot).
pub fn register_all(
    state: &crate::location::ToolRegistryState,
    hooks: &Arc<crate::hooks::HookRegistry>,
    services: &Arc<ToolServices>,
) -> Result<(), String> {
    for make in [
        patch::tool as fn(&Arc<ToolServices>) -> BuiltinTool,
        edit::tool,
        glob::tool,
        grep::tool,
        read::tool,
        write::tool,
    ] {
        register_builtin(state, hooks, services, make(services))?;
    }
    patch::install_codemod_hooks(hooks);
    Ok(())
}

/// The six filesystem-tool internal plugins, in the v2 `pre` boot order
/// (internal.ts: Patch, Edit, Glob, Grep, …, Read, …, Write).
#[must_use]
pub fn builtin_plugins(services: Arc<ToolServices>) -> Vec<InternalPlugin> {
    let leaves: Vec<fn(&Arc<ToolServices>) -> BuiltinTool> =
        vec![patch::tool, edit::tool, glob::tool, grep::tool, read::tool, write::tool];
    let mut plugins = Vec::new();
    for make in leaves {
        let services = Arc::clone(&services);
        let id = format!("opencode.tool.{}", make(&services).name);
        plugins.push(InternalPlugin::new(id, move |hooks, ports| {
            let tool = make(&services);
            register(hooks, ports, &services, tool)
        }));
    }
    // The patch plugin's session hooks (gpt-* codemod tool removal).
    plugins.push(patch::plugin());
    plugins
}

/// The registration step shared by the plugin closure and direct callers:
/// registers the tool + wrapped executor, then the codemod hooks.
fn register(
    hooks: &Arc<crate::hooks::HookRegistry>,
    ports: &crate::location::LocationPorts<'_>,
    services: &Arc<ToolServices>,
    tool: BuiltinTool,
) -> Result<(), String> {
    register_builtin(ports.tools, hooks, services, tool)
}

/// Shared permission-assert helper (every leaf builds the same source shape).
pub(crate) fn tool_source(context: &ToolCallContext) -> Option<oc_schema::permission::Source> {
    Some(oc_schema::permission::Source::Tool {
        message_id: context.message_id.clone(),
        id: context.call_id.clone(),
    })
}

/// Re-exported context type for leaf signatures.
pub use crate::location::ToolCallContext as Context;

#[cfg(test)]
mod tests;
