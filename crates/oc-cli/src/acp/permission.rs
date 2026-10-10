//! Permission bridging (port of `acp/permission.ts`): opencode
//! `permission.asked` → ACP `session/request_permission`, with diff
//! previews and the allow-once/allow-always/reject options.

use agent_client_protocol::schema::v1::{
    PermissionOption, PermissionOptionKind, RequestPermissionOutcome, RequestPermissionRequest,
    ToolCallContent, ToolCallLocation,
};
use serde_json::{Map, Value};

use crate::acp::child::{self, ChildSession};
use crate::acp::error::{classify, AcpError};
use crate::acp::tool::{self, PatchHunk};
use crate::acp::Conn;

/// `options` (permission.ts:38-42).
#[must_use]
pub fn options() -> Vec<PermissionOption> {
    vec![
        option("once", PermissionOptionKind::AllowOnce, "Allow once"),
        option("always", PermissionOptionKind::AllowAlways, "Always allow"),
        option("reject", PermissionOptionKind::RejectOnce, "Reject"),
    ]
}

fn option(id: &str, kind: PermissionOptionKind, name: &str) -> PermissionOption {
    PermissionOption::new(id.to_owned(), name.to_owned(), kind)
}

/// The ask input (permission.ts:27-36).
pub struct AskInput<'a> {
    pub client: &'a oc_client::Client,
    pub conn: &'a Conn,
    /// The raw `permission.asked` data.
    pub event: &'a Value,
    pub session_id: String,
    pub client_session_id: String,
    pub cwd: String,
    pub tracked_tool: Option<crate::acp::translate::Tool>,
    pub child: Option<ChildSession>,
}

/// The decision fed back to the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Once,
    Always,
    Reject,
}

/// `ask` (permission.ts:46-72): build the pending tool call with
/// previews and title, ask the client, map the outcome. `cancelled`
/// resolves to the reject fallback (turn.ts:118-131).
pub async fn ask(
    input: &AskInput<'_>,
    cancelled: &mut (impl std::future::Future<Output = ()> + Unpin),
) -> Result<Decision, AcpError> {
    let tool_name = input
        .tracked_tool
        .as_ref()
        .map_or_else(|| string(input.event, "action"), |tool| tool.name.clone());
    let tool_input: Map<String, Value> = input
        .tracked_tool
        .as_ref()
        .map(|tool| tool.input.clone())
        .or_else(|| input.event.get("metadata").and_then(Value::as_object).cloned())
        .unwrap_or_default();
    let previews =
        permission_previews(&tool_name, &tool_input, input.event.get("metadata"), &input.cwd);
    let title = permission_title(&tool_name, &tool_input, &previews);
    let tool_call_id = child::tool_call_id(
        input.child.as_ref(),
        &input
            .tracked_tool
            .as_ref()
            .map_or_else(|| string(input.event, "id"), |tool| tool.id.clone()),
    );
    let state_title = if title.is_some() {
        child::prefix_title(input.child.as_ref(), &title.clone().unwrap())
    } else {
        input.child.as_ref().and_then(|child| child.title.clone()).unwrap_or_default()
    };
    let metadata_meta: Option<Map<String, Value>> = input
        .child
        .as_ref()
        .and_then(|child| serde_json::to_value(child::meta(child)).ok())
        .and_then(|value| serde_json::from_value(value).ok());
    let locations = permission_locations(&tool_name, &tool_input, input.event, &input.cwd);
    let raw_input_override = if input.tracked_tool.is_some() {
        &tool::RawInputOverride::Keep
    } else {
        &tool::RawInputOverride::Omit
    };
    let content: Vec<ToolCallContent> = previews
        .iter()
        .map(|preview| {
            ToolCallContent::Diff(
                agent_client_protocol::schema::v1::Diff::new(
                    preview.path.clone(),
                    preview.new_text.clone(),
                )
                .old_text(preview.old_text.clone()),
            )
        })
        .collect();
    let tool_call_update = tool::pending_tool_update(
        &tool_call_id,
        &tool_name,
        &tool_input,
        (!state_title.is_empty()).then_some(state_title.as_str()),
        &input.cwd,
        metadata_meta,
        raw_input_override,
        Some(locations),
        content,
    );
    let request =
        RequestPermissionRequest::new(input.client_session_id.clone(), tool_call_update, options());

    // Race the ask against cancellation; an earlier cancel never starts
    // the ask (turn.ts:118-124) and falls back to reject.
    let sent = input.conn.send_request(request);
    let outcome = tokio::select! {
        () = &mut *cancelled => return Ok(Decision::Reject),
        outcome = sent.block_task() => outcome.map_err(|request_error| {
            AcpError::ServiceFailure {
                safe_message: format!("permission request failed: {request_error}"),
                service: Some("session".to_owned()),
                error_name: Some("RequestPermission".to_owned()),
            }
        })?,
    };
    let RequestPermissionOutcome::Selected(selected) = outcome.outcome else {
        return Ok(Decision::Reject);
    };
    Ok(match selected.option_id.0.as_ref() {
        "once" => Decision::Once,
        "always" => Decision::Always,
        _ => Decision::Reject,
    })
}

/// `respond` (permission.ts:74-79).
pub async fn respond(input: &AskInput<'_>, decision: Decision) -> Result<(), AcpError> {
    let reply = match decision {
        Decision::Once => oc_client::types::PermissionReply::Once,
        Decision::Always => oc_client::types::PermissionReply::Always,
        Decision::Reject => oc_client::types::PermissionReply::Reject,
    };
    let result = input
        .client
        .session()
        .permission_reply(
            &input.session_id,
            &string(input.event, "id"),
            &oc_client::types::SessionPermissionReplyBody { decision: reply, message: None },
        )
        .await;
    match result {
        Ok(()) => Ok(()),
        // PermissionNotFoundError → the ask already settled (permission.ts:76).
        Err(error) if error.api_name() == Some("PermissionNotFoundError") => Ok(()),
        Err(error) => Err(classify(&error)),
    }
}

/// One diff preview (`diff`, permission.ts:123-125).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    pub path: String,
    pub old_text: Option<String>,
    pub new_text: String,
}

/// `permissionPreviews` (permission.ts:82-102).
fn permission_previews(
    tool_name: &str,
    input: &Map<String, Value>,
    metadata: Option<&Value>,
    cwd: &str,
) -> Vec<Preview> {
    if tool::canonical_name(tool_name) == "patch" {
        return patch_previews(input, cwd);
    }
    let Some(files) = metadata.and_then(|metadata| metadata.get("files")).and_then(Value::as_array)
    else {
        return Vec::new();
    };
    let mut previews = Vec::new();
    for file in files {
        let path = tool::absolute_path(&string(file, "file"), cwd);
        let status = string(file, "status");
        let patch_text = string(file, "patch");
        let old_text = if status == "added" { None } else { std::fs::read_to_string(&path).ok() };
        let Some(old_text) = old_text else {
            continue;
        };
        let Some(new_text) = apply_unified(&old_text, &patch_text) else {
            continue;
        };
        previews.push(Preview { path, old_text: Some(old_text), new_text });
    }
    previews
}

/// `patchPreviews` (permission.ts:104-121): the patch tool's previews
/// come from its own hunks (core trims the diffs for display).
fn patch_previews(input: &Map<String, Value>, cwd: &str) -> Vec<Preview> {
    let hunks: Vec<PatchHunk> = tool::patch_hunks(input);
    let mut previews = Vec::new();
    for hunk in hunks {
        let path = tool::absolute_path(&hunk.path, cwd);
        if hunk.r#type == "add" {
            let new_text = if hunk.contents.ends_with('\n') || hunk.contents.is_empty() {
                hunk.contents.clone()
            } else {
                format!("{}\n", hunk.contents)
            };
            previews.push(Preview { path, old_text: None, new_text });
            continue;
        }
        let Some(old_text) = std::fs::read_to_string(&path).ok() else { continue };
        if hunk.r#type == "delete" {
            previews.push(Preview { path, old_text: Some(old_text), new_text: String::new() });
            continue;
        }
        let Ok(derived) = oc_core::tool::patch_util::derive(&hunk.path, &hunk.chunks, &old_text)
        else {
            continue;
        };
        let target =
            hunk.move_path.as_ref().map(|moved| tool::absolute_path(moved, cwd)).unwrap_or(path);
        previews.push(Preview {
            path: target,
            old_text: Some(old_text),
            new_text: derived.content,
        });
    }
    previews
}

/// A minimal unified-diff applier (the `diff` package's `applyPatch`
/// role, permission.ts:96): applies `@@ -a,b +c,d @@` hunks with
/// context lines. Returns `None` on any mismatch.
pub fn apply_unified(old_text: &str, patch: &str) -> Option<String> {
    let old_lines: Vec<&str> = old_text.split('\n').collect();
    let mut result: Vec<String> = Vec::new();
    let mut old_index = 0usize;
    let mut hunks = patch.lines().peekable();
    // Skip the file header.
    while let Some(line) = hunks.peek() {
        if line.starts_with("--- ") || line.starts_with("+++ ") || line.starts_with("diff ") {
            hunks.next();
        } else {
            break;
        }
    }
    let mut any = false;
    while let Some(line) = hunks.next() {
        let header = line.strip_prefix("@@")?;
        let header = header.strip_suffix("@@").unwrap_or(header.trim_end());
        let header = header.trim();
        let (old_start, _old_count) = parse_hunk_header(header)?;
        any = true;
        // Unified hunks are 1-based; a 0 start means an insertion point.
        let mut position = if old_start == 0 { 0 } else { old_start - 1 };
        if position > old_index {
            result
                .extend(old_lines.get(old_index..position)?.iter().map(|line| (*line).to_owned()));
            old_index = position;
        }
        while let Some(next) = hunks.peek() {
            if next.starts_with("@@") {
                break;
            }
            let change = hunks.next().expect("peeked");
            match change.chars().next() {
                Some(' ') => {
                    let expected = old_lines.get(position)?;
                    if expected.trim_end() != change[1..].trim_end() {
                        return None;
                    }
                    result.push(change[1..].to_owned());
                    position += 1;
                    old_index = position;
                }
                Some('-') => {
                    let expected = old_lines.get(position)?;
                    if expected.trim_end() != change[1..].trim_end() {
                        return None;
                    }
                    position += 1;
                    old_index = position;
                }
                Some('+') => result.push(change[1..].to_owned()),
                Some('\\') => {} // "\ No newline at end of file"
                _ => return None,
            }
        }
    }
    if !any {
        return None;
    }
    result.extend(old_lines.get(old_index..)?.iter().map(|line| (*line).to_owned()));
    Some(result.join("\n"))
}

fn parse_hunk_header(header: &str) -> Option<(usize, usize)> {
    // "-l,s +l,s" with the counts optional.
    let mut parts = header.split_whitespace();
    let old = parts.next()?.strip_prefix('-')?;
    let (start, count) = match old.split_once(',') {
        Some((start, count)) => (start.parse::<usize>().ok()?, count.parse::<usize>().ok()?),
        None => (old.parse::<usize>().ok()?, 1),
    };
    Some((start, count))
}

/// `permissionTitle` (permission.ts:127-147).
fn permission_title(
    tool_name: &str,
    input: &Map<String, Value>,
    previews: &[Preview],
) -> Option<String> {
    if previews.len() > 1 {
        return Some(format!("{} files", previews.len()));
    }
    match tool::canonical_name(tool_name).as_str() {
        "external_directory" => input
            .get("description")
            .and_then(Value::as_str)
            .or_else(|| input.get("command").and_then(Value::as_str))
            .or_else(|| input.get("parentDir").and_then(Value::as_str))
            .map(str::to_owned),
        "webfetch" => input.get("url").and_then(Value::as_str).map(str::to_owned),
        "websearch" => input.get("query").and_then(Value::as_str).map(str::to_owned),
        "grep" | "glob" => input.get("pattern").and_then(Value::as_str).map(str::to_owned),
        "read" | "edit" | "write" | "patch" => input
            .get("path")
            .and_then(Value::as_str)
            .or_else(|| input.get("filePath").and_then(Value::as_str))
            .map(str::to_owned)
            .or_else(|| previews.first().map(|preview| preview.path.clone())),
        _ => None,
    }
}

/// `permissionLocations` (permission.ts:149-157).
fn permission_locations(
    tool_name: &str,
    input: &Map<String, Value>,
    ask: &Value,
    cwd: &str,
) -> Vec<ToolCallLocation> {
    let locations = tool::to_locations(tool_name, input, cwd);
    if !locations.is_empty() || !matches!(tool_name, "read" | "edit" | "external_directory") {
        return locations;
    }
    let mut paths: Vec<String> = Vec::new();
    let empty: Vec<Value> = Vec::new();
    for resource in ask.get("resources").and_then(Value::as_array).unwrap_or(&empty) {
        let Some(resource) = resource.as_str() else { continue };
        let path = resource.strip_suffix("/*").unwrap_or(resource);
        if !path.is_empty() && !path.contains(['*', '?']) && !paths.contains(&path.to_owned()) {
            paths.push(tool::absolute_path(path, cwd));
        }
    }
    paths.into_iter().map(ToolCallLocation::new).collect()
}

fn string(value: &Value, field: &str) -> String {
    value.get(field).and_then(Value::as_str).unwrap_or_default().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_match_v2_exactly() {
        let options = options();
        let ids: Vec<String> =
            options.iter().map(|option| option.option_id.0.as_ref().to_owned()).collect();
        assert_eq!(ids, vec!["once".to_owned(), "always".to_owned(), "reject".to_owned()]);
        assert!(matches!(options[0].kind, PermissionOptionKind::AllowOnce));
        assert!(matches!(options[1].kind, PermissionOptionKind::AllowAlways));
        assert!(matches!(options[2].kind, PermissionOptionKind::RejectOnce));
        assert_eq!(options[0].name, "Allow once");
        assert_eq!(options[1].name, "Always allow");
        assert_eq!(options[2].name, "Reject");
    }

    #[test]
    fn unified_diffs_apply() {
        let old = "line one\nline two\nline three";
        let patch = "@@ -1,3 +1,3 @@\n line one\n-line two\n+line 2\n line three";
        assert_eq!(apply_unified(old, patch).unwrap(), "line one\nline 2\nline three");
        // Header lines are skipped.
        let patch = "--- a\n+++ b\n@@ -1,1 +1,1 @@\n-line one\n+first";
        assert_eq!(apply_unified(old, patch).unwrap(), "first\nline two\nline three");
        // A mismatched context fails cleanly.
        let bad = "@@ -1,2 +1,2 @@\n nope\n-x\n+y";
        assert!(apply_unified(old, bad).is_none());
    }

    #[test]
    fn titles_follow_the_tool_table() {
        let previews = vec![Preview {
            path: "/w/f.txt".to_owned(),
            old_text: Some("a".to_owned()),
            new_text: "b".to_owned(),
        }];
        let input = serde_json::json!({ "path": "f.txt" });
        let input = input.as_object().cloned().unwrap();
        assert_eq!(permission_title("edit", &input, &previews).as_deref(), Some("f.txt"));
        let multi = vec![previews[0].clone(), previews[0].clone()];
        assert_eq!(permission_title("edit", &input, &multi).as_deref(), Some("2 files"));
    }
}
