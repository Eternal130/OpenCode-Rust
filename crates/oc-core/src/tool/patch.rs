//! Port of `tool/plugin/patch.ts` — the multi-hunk patch leaf: parse →
//! prepare (add trailing newline, cumulative update chain, move targets) →
//! ONE `edit` permission assert over every resource → sequential writes
//! with partial-failure reporting → formatter resync (core-runner-map §4.8).
//! The `opencode.tool.patch` plugin also registers the gpt-*
//! edit/write/patch codemod removal on the context/compaction/generate
//! hooks.

use std::path::PathBuf;

use oc_schema::session_support::{FileDiff, FileDiffStatus};
use oc_schema::tool::Content;
use serde_json::Value;

use super::file_diff::file_diff;
use super::patch_util::{self, Hunk};
use super::{BuiltinTool, Context, ToolFailure, ToolOutcome, ToolServices};
use crate::file_access::{Invocation, Kind, ResolveInput, Target};
use crate::hooks::fn_hook;

/// The verbatim `patch.txt` description.
pub fn description() -> String {
    include_str!("patch.txt").to_string()
}

pub fn schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "patchText": {
                "type": "string",
                "description": "The full patch text describing add, update, and delete operations"
            }
        },
        "required": ["patchText"]
    })
}

pub fn tool(_services: &std::sync::Arc<ToolServices>) -> BuiltinTool {
    BuiltinTool {
        name: "patch",
        description: description(),
        input: schema(),
        permission: Some("edit"),
        codemode: false,
        namespace: None,
        execute: std::sync::Arc::new(PatchExecutor),
    }
}

/// The `opencode.tool.patch` internal plugin: the gpt-* codemod hook on
/// the context/compaction/generate points (the tool registration itself
/// goes through [`super::builtin_plugins`]).
pub fn plugin() -> crate::hooks::InternalPlugin {
    crate::hooks::InternalPlugin::new("opencode.tool.patch-hooks", |hooks, _ports| {
        install_codemod_hooks(hooks);
        Ok(())
    })
}

/// One prepared file change.
struct Prepared {
    kind: &'static str, // "add" | "update" | "delete"
    target: Target,
    content: String,
    before: String,
    after: String,
    move_target: Option<Target>,
}

struct PatchExecutor;

#[async_trait::async_trait]
impl super::ToolExecuteWithContext for PatchExecutor {
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

/// `toModelContent`.
#[must_use]
pub fn to_model_content(applied: &[(&'static str, &str)]) -> String {
    let mut lines = vec!["Success. Updated the following files:".to_string()];
    for (kind, resource) in applied {
        let marker = match *kind {
            "add" => "A",
            "delete" => "D",
            _ => "M",
        };
        lines.push(format!("{marker} {resource}"));
    }
    lines.join("\n")
}

fn error_message(error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::NotFound => "file does not exist".to_string(),
        std::io::ErrorKind::IsADirectory | std::io::ErrorKind::Other => {
            if error.raw_os_error() == Some(21) {
                "path is a directory".to_string()
            } else {
                error.to_string()
            }
        }
        _ => error.to_string(),
    }
}

/// `patchFile` + `trimDiff` (de-indent the common whitespace).
fn patch_file(prepared: &Prepared, after: Option<&str>) -> FileDiff {
    let target = prepared.move_target.as_ref().map_or(&prepared.target, |t| t);
    let status = match prepared.kind {
        "add" => FileDiffStatus::Added,
        "delete" => FileDiffStatus::Deleted,
        _ => FileDiffStatus::Modified,
    };
    let mut diff = file_diff(
        &prepared.target.absolute.to_string_lossy(),
        &prepared.before,
        after.unwrap_or(&prepared.after),
        status,
    );
    diff.file.clone_from(&target.resource);
    diff.patch = trim_diff(&diff.patch);
    diff
}

fn trim_diff(diff: &str) -> String {
    let lines: Vec<&str> = diff.split('\n').collect();
    let content: Vec<&&str> = lines
        .iter()
        .filter(|line| {
            (line.starts_with('+') || line.starts_with('-') || line.starts_with(' '))
                && !line.starts_with("---")
                && !line.starts_with("+++")
        })
        .collect();
    if content.is_empty() {
        return diff.to_string();
    }
    let indent = content.iter().fold(usize::MAX, |result, line| {
        let value = line[1..].strip_suffix('\r').unwrap_or(&line[1..]);
        if value.trim().is_empty() {
            return result;
        }
        let leading = value.len() - value.trim_start_matches([' ', '\t']).len();
        result.min(leading)
    });
    if indent == usize::MAX || indent == 0 {
        return diff.to_string();
    }
    lines
        .into_iter()
        .map(|line| {
            if (line.starts_with('+') || line.starts_with('-') || line.starts_with(' '))
                && !line.starts_with("---")
                && !line.starts_with("+++")
            {
                let (head, rest) = line.split_at(1);
                format!("{head}{}", rest.get(indent..).unwrap_or(""))
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

async fn run(
    services: &ToolServices,
    input: &Value,
    context: &Context,
) -> Result<ToolOutcome, ToolFailure> {
    let patch_text = input["patchText"].as_str().unwrap_or_default().to_string();
    if patch_text.is_empty() {
        return Err(ToolFailure::message("patchText is required"));
    }
    let hunks = patch_util::parse(&patch_text)
        .map_err(|error| ToolFailure::message(format!("patch verification failed: {error}")))?;
    if hunks.is_empty() {
        return Err(ToolFailure::message("patch rejected: empty patch"));
    }

    let lock_targets: Vec<String> = hunks
        .iter()
        .flat_map(|hunk| {
            let mut targets =
                vec![services.mutation.resolve_path(hunk.path()).to_string_lossy().into_owned()];
            if let Hunk::Update { move_path: Some(move_path), .. } = hunk {
                targets
                    .push(services.mutation.resolve_path(move_path).to_string_lossy().into_owned());
            }
            targets
        })
        .collect();
    services.mutation.with_lock(&lock_targets, execute_patch(services, &hunks, context)).await
}

/// The partial-failure helper: `{op}: {error}. Completed before failure: …`.
fn fail(operation: &str, error: &str, applied: &[String]) -> ToolFailure {
    let completed = applied.join(", ");
    ToolFailure::message(format!(
        "{operation}: {error}{}",
        if completed.is_empty() {
            String::new()
        } else {
            format!(". Completed before failure: {completed}")
        }
    ))
}

async fn execute_patch(
    services: &ToolServices,
    hunks: &[Hunk],
    context: &Context,
) -> Result<ToolOutcome, ToolFailure> {
    let invocation = Invocation {
        session_id: context.session_id.clone(),
        agent: Some(context.agent.clone()),
        message_id: context.message_id.clone(),
        id: context.call_id.clone(),
    };

    // ---- prepare ---------------------------------------------------------
    let mut prepared: Vec<Prepared> = Vec::new();
    let mut updates: std::collections::HashMap<PathBuf, String> = std::collections::HashMap::new();
    for hunk in hunks {
        let target = services
            .access
            .resolve(&ResolveInput { path: hunk.path().to_string(), kind: Some(Kind::File) });
        if let Some(auth) = &target.external_directory {
            // External approval carries filepath/parentDir metadata.
            let metadata = serde_json::from_value(serde_json::json!({
                "filepath": target.absolute.to_string_lossy().replace('\\', "/"),
                "parentDir": auth.directory.to_string_lossy().replace('\\', "/"),
            }))
            .unwrap_or_default();
            services
                .access
                .authorize_external(std::slice::from_ref(&target), &invocation, Some(metadata))
                .await
                .map_err(|error| ToolFailure {
                    message: format!("Unable to prepare patch at {}", hunk.path()),
                    cause: Some(super::permission_cause(&error)),
                })?;
        }
        match hunk {
            Hunk::Add { contents, .. } => {
                let content = if contents.ends_with('\n') || contents.is_empty() {
                    contents.clone()
                } else {
                    format!("{contents}\n")
                };
                let after = crate::file_mutation::bom_split(&content).1.to_string();
                prepared.push(Prepared {
                    kind: "add",
                    target,
                    content,
                    before: String::new(),
                    after,
                    move_target: None,
                });
            }
            Hunk::Delete { .. } => {
                let content =
                    crate::file_mutation::read_text(&target.absolute).map_err(|error| {
                        ToolFailure::message(format!(
                            "patch verification failed: Failed to delete {}: {}",
                            target.resource,
                            error_message(&error)
                        ))
                    })?;
                prepared.push(Prepared {
                    kind: "delete",
                    target,
                    content: String::new(),
                    before: content.text,
                    after: String::new(),
                    move_target: None,
                });
            }
            Hunk::Update { chunks, move_path, .. } => {
                let previous = updates.get(&target.absolute);
                let original = match previous {
                    Some(previous) => previous.clone(),
                    None => crate::file_mutation::read_text(&target.absolute)
                        .map_err(|error| {
                            ToolFailure::message(format!(
                                "patch verification failed: Failed to read file to update {}: {}",
                                target.absolute.display(),
                                error_message(&error)
                            ))
                        })
                        .map(|content| {
                            crate::file_mutation::bom_join(&content.text, content.bom)
                        })?,
                };
                let before = crate::file_mutation::bom_split(&original).1.to_string();
                let target_absolute = target.absolute.clone();
                let update =
                    patch_util::derive(hunk.path(), chunks, &original).map_err(|error| {
                        ToolFailure::message(format!("patch verification failed: {error}"))
                    })?;
                let joined = patch_util::join_bom(&update.content, update.bom);
                let move_target = match move_path {
                    Some(move_path) => {
                        let resolved = services.access.resolve(&ResolveInput {
                            path: move_path.clone(),
                            kind: Some(Kind::File),
                        });
                        if let Some(auth) = &resolved.external_directory {
                            let metadata = serde_json::from_value(serde_json::json!({
                                "filepath": resolved.absolute.to_string_lossy().replace('\\', "/"),
                                "parentDir": auth.directory.to_string_lossy().replace('\\', "/"),
                            }))
                            .unwrap_or_default();
                            services
                                .access
                                .authorize_external(
                                    std::slice::from_ref(&resolved),
                                    &invocation,
                                    Some(metadata),
                                )
                                .await
                                .map_err(|error| ToolFailure {
                                    message: format!("Unable to prepare patch at {}", hunk.path()),
                                    cause: Some(super::permission_cause(&error)),
                                })?;
                        }
                        Some(resolved)
                    }
                    None => None,
                };
                let has_move = move_target.is_some();
                prepared.push(Prepared {
                    kind: "update",
                    target,
                    content: joined.clone(),
                    before,
                    after: update.content,
                    move_target,
                });
                if !has_move {
                    updates.insert(target_absolute, joined);
                }
            }
        }
    }

    // ---- ONE permission assert over every resource -------------------------
    let patch_files: Vec<FileDiff> = prepared.iter().map(patch_file_as_is).collect();
    let mut resources: Vec<String> = prepared
        .iter()
        .flat_map(|change| {
            let mut targets = vec![change.target.resource.clone()];
            if let Some(move_target) = &change.move_target {
                targets.push(move_target.resource.clone());
            }
            targets
        })
        .collect();
    resources.sort();
    resources.dedup();
    let mut assert_input =
        crate::permission::AssertInput::new(context.session_id.clone(), "edit", resources.clone());
    assert_input.agent = Some(context.agent.clone());
    assert_input.save = Some(vec!["*".to_string()]);
    let diffs_joined: String = patch_files.iter().map(|file| format!("{}\n", file.patch)).collect();
    assert_input.metadata = Some(
        serde_json::from_value(serde_json::json!({
            "filepath": resources.join(", "),
            "diff": diffs_joined,
            "files": patch_files,
        }))
        .unwrap_or_default(),
    );
    assert_input.source = super::tool_source(context);
    if let Err(error) = services.permission.assert(assert_input).await {
        return Err(ToolFailure {
            message: "Unable to apply patch".to_string(),
            cause: Some(super::permission_cause(&error)),
        });
    }

    // ---- sequential writes, no rollback ------------------------------------
    let mut applied: Vec<(&'static str, String, PathBuf)> = Vec::new();
    for change in &prepared {
        match change.kind {
            "delete" => {
                std::fs::remove_file(&change.target.absolute).map_err(|error| {
                    fail(
                        &format!("Failed to delete {}", change.target.resource),
                        &error_message(&error),
                        &applied.iter().map(|(_, r, _)| r.clone()).collect::<Vec<_>>(),
                    )
                })?;
                applied.push((
                    "delete",
                    change.target.resource.clone(),
                    change.target.absolute.clone(),
                ));
            }
            "update" if change.move_target.is_some() => {
                let move_target =
                    change.move_target.clone().unwrap_or_else(|| change.target.clone());
                std::fs::write(&move_target.absolute, change.content.as_bytes()).map_err(
                    |error| {
                        fail(
                            &format!("Failed to write {}", move_target.resource),
                            &error_message(&error),
                            &applied.iter().map(|(_, r, _)| r.clone()).collect::<Vec<_>>(),
                        )
                    },
                )?;
                std::fs::remove_file(&change.target.absolute).map_err(|error| {
                    fail(
                        &format!(
                            "Wrote {} but failed to remove {}",
                            move_target.resource, change.target.resource
                        ),
                        &error_message(&error),
                        &applied.iter().map(|(_, r, _)| r.clone()).collect::<Vec<_>>(),
                    )
                })?;
                applied.push((
                    "update",
                    move_target.resource.clone(),
                    move_target.absolute.clone(),
                ));
            }
            kind => {
                if let Some(parent) = change.target.absolute.parent() {
                    std::fs::create_dir_all(parent).map_err(|error| {
                        fail(
                            &format!("Failed to write {}", change.target.resource),
                            &error_message(&error),
                            &applied.iter().map(|(_, r, _)| r.clone()).collect::<Vec<_>>(),
                        )
                    })?;
                }
                std::fs::write(&change.target.absolute, change.content.as_bytes()).map_err(
                    |error| {
                        fail(
                            &format!("Failed to write {}", change.target.resource),
                            &error_message(&error),
                            &applied.iter().map(|(_, r, _)| r.clone()).collect::<Vec<_>>(),
                        )
                    },
                )?;
                applied.push((
                    kind,
                    change.target.resource.clone(),
                    change.target.absolute.clone(),
                ));
            }
        }
    }

    // ---- formatter resync ---------------------------------------------------
    let mut formatted: std::collections::HashMap<PathBuf, String> =
        std::collections::HashMap::new();
    for (_, _, absolute) in &applied {
        let Ok(current) = crate::file_mutation::read_text(absolute) else { continue };
        let text = if services.formatter.file(absolute) {
            crate::file_mutation::sync_text_bom(absolute, current.bom)
                .map(|content| content.text)
                .unwrap_or(current.text)
        } else {
            current.text
        };
        formatted.insert(absolute.clone(), text);
    }
    let files: Vec<FileDiff> = prepared
        .iter()
        .map(|change| {
            if change.kind == "delete" {
                return patch_file(change, None);
            }
            let target = change.move_target.as_ref().map_or(&change.target, |t| t);
            patch_file(change, formatted.get(&target.absolute).map(String::as_str))
        })
        .collect();

    Ok(ToolOutcome {
        content: vec![Content::Text {
            text: to_model_content(
                &applied.iter().map(|(k, r, _)| (*k, r.as_str())).collect::<Vec<_>>(),
            ),
        }],
        metadata: Some(
            serde_json::from_value(serde_json::json!({ "files": files })).unwrap_or_default(),
        ),
        output: Some(serde_json::json!({
            "applied": applied.iter().map(|(kind, resource, target)| serde_json::json!({
                "type": kind,
                "resource": resource,
                "target": target.to_string_lossy().replace('\\', "/"),
            })).collect::<Vec<_>>(),
            "files": files,
        })),
    })
}

fn patch_file_as_is(change: &Prepared) -> FileDiff {
    patch_file(change, None)
}

/// Installs the gpt-* codemod hook on the context/compaction/generate
/// points (registration closures are sync; the registry's async surface
/// resolves immediately — see the Wave 0 supervisor test).
pub(crate) fn install_codemod_hooks(hooks: &crate::hooks::HookRegistry) {
    use crate::hooks::{HookOptions, SessionCompactionEvent, SessionContextEvent};
    let handler = fn_hook(|event: &mut SessionContextEvent| {
        let use_patch = super::edit::use_patch_tool(&event.model.id.0);
        if use_patch {
            event.tools.remove("edit");
            event.tools.remove("write");
        } else {
            event.tools.remove("patch");
        }
        Ok(())
    });
    let compaction_handler = fn_hook(|event: &mut SessionCompactionEvent| {
        let use_patch = super::edit::use_patch_tool(&event.context.model.id.0);
        if use_patch {
            event.context.tools.remove("edit");
            event.context.tools.remove("write");
        } else {
            event.context.tools.remove("patch");
        }
        Ok(())
    });
    // `session.generate` shares the SessionContextEvent shape.
    let generate_handler = fn_hook(|event: &mut SessionContextEvent| {
        let use_patch = super::edit::use_patch_tool(&event.model.id.0);
        if use_patch {
            event.tools.remove("edit");
            event.tools.remove("write");
        } else {
            event.tools.remove("patch");
        }
        Ok(())
    });
    let options = HookOptions::default();
    // Registrations live for the location lifetime (dropped with the graph).
    std::mem::forget(futures::executor::block_on(
        hooks.register_session_context(handler, options.clone()),
    ));
    std::mem::forget(futures::executor::block_on(
        hooks.register_session_generate(generate_handler, options.clone()),
    ));
    std::mem::forget(futures::executor::block_on(
        hooks.register_session_compaction(compaction_handler, options),
    ));
}
