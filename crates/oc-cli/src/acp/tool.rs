//! Tool-name mapping and tool-call shaping (port of `acp/tool.ts`):
//! opencode tool names → ACP `ToolKind`, titles, locations, raw input,
//! and the pending/running/completed/error tool-call updates.

use std::path::{Path, PathBuf};

use agent_client_protocol::schema::v1::{
    Content as ToolWrap, ContentBlock, Diff, TextContent, ToolCall, ToolCallContent,
    ToolCallLocation, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
};
use serde_json::{Map, Value};

/// `toToolKind` (tool.ts:10-33).
#[must_use]
pub fn to_tool_kind(tool_name: &str) -> ToolKind {
    match canonical_name(tool_name).as_str() {
        "shell" => ToolKind::Execute,
        "webfetch" => ToolKind::Fetch,
        "edit" | "patch" | "write" => ToolKind::Edit,
        "grep"
        | "glob"
        | "context"
        | "context7_resolve_library_id"
        | "context7_get_library_docs" => ToolKind::Search,
        "read" => ToolKind::Read,
        "subagent" => ToolKind::Think,
        _ => ToolKind::Other,
    }
}

/// `V1Aliases` + `canonicalName` (tool.ts:181-190): lower-case and map
/// the v1-era aliases.
#[must_use]
pub fn canonical_name(tool_name: &str) -> String {
    let name = tool_name.to_lowercase();
    match name.as_str() {
        "bash" => "shell".to_owned(),
        "task" => "subagent".to_owned(),
        "apply_patch" => "patch".to_owned(),
        other => other.to_owned(),
    }
}

fn string_value(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_owned)
}

/// `filePath` (tool.ts:193-195): sessions migrated from V1 keep their
/// original `filePath` tool inputs.
fn file_path(input: &Map<String, Value>) -> Option<String> {
    string_value(input.get("path")).or_else(|| string_value(input.get("filePath")))
}

/// `absolutePath` (tool.ts:204-206).
#[must_use]
pub fn absolute_path(path: &str, cwd: &str) -> String {
    if Path::new(path).is_absolute() {
        path.to_owned()
    } else {
        Path::new(cwd).join(path).to_string_lossy().into_owned()
    }
}

/// `locationFrom` (tool.ts:174-179): de-duplicated absolute paths.
fn location_from(cwd: &str, values: Vec<Option<String>>) -> Vec<ToolCallLocation> {
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut locations = Vec::new();
    for value in values.into_iter().flatten() {
        if value.is_empty() {
            continue;
        }
        let path = PathBuf::from(absolute_path(&value, cwd));
        if !seen.contains(&path) {
            seen.push(path.clone());
            locations.push(ToolCallLocation::new(path));
        }
    }
    locations
}

/// `toLocations` (tool.ts:35-59).
#[must_use]
pub fn to_locations(
    tool_name: &str,
    input: &Map<String, Value>,
    cwd: &str,
) -> Vec<ToolCallLocation> {
    match canonical_name(tool_name).as_str() {
        "shell" => location_from(
            cwd,
            vec![
                string_value(input.get("workdir")).or_else(|| string_value(input.get("cwd"))),
                Some(cwd.to_owned()),
            ],
        ),
        "read" | "edit" | "write" => location_from(cwd, vec![file_path(input)]),
        "patch" => location_from(
            cwd,
            patch_hunks(input)
                .into_iter()
                .flat_map(|hunk| {
                    let moved = if hunk.r#type == "update" { hunk.move_path } else { None };
                    vec![Some(hunk.path), moved]
                })
                .collect(),
        ),
        "external_directory" => location_from(cwd, vec![string_value(input.get("filepath"))]),
        "grep"
        | "glob"
        | "context"
        | "context7_resolve_library_id"
        | "context7_get_library_docs" => location_from(cwd, vec![string_value(input.get("path"))]),
        _ => Vec::new(),
    }
}

/// One parsed hunk of the patch tool's input (`patchHunks`, tool.ts:197-202).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchHunk {
    pub r#type: String,
    pub path: String,
    pub move_path: Option<String>,
    pub contents: String,
    pub chunks: Vec<oc_core::tool::patch_util::UpdateFileChunk>,
}

/// `patchHunks` (tool.ts:197-202) over `Patch.parse` — the same parser
/// the server's patch tool uses (`oc_core::tool::patch_util`).
#[must_use]
pub fn patch_hunks(input: &Map<String, Value>) -> Vec<PatchHunk> {
    let Some(patch_text) = string_value(input.get("patchText")) else {
        return Vec::new();
    };
    let Ok(hunks) = oc_core::tool::patch_util::parse(&patch_text) else {
        return Vec::new();
    };
    hunks
        .into_iter()
        .map(|hunk| match hunk {
            oc_core::tool::patch_util::Hunk::Add { path, contents } => PatchHunk {
                r#type: "add".to_owned(),
                path,
                move_path: None,
                contents,
                chunks: Vec::new(),
            },
            oc_core::tool::patch_util::Hunk::Delete { path } => PatchHunk {
                r#type: "delete".to_owned(),
                path,
                move_path: None,
                contents: String::new(),
                chunks: Vec::new(),
            },
            oc_core::tool::patch_util::Hunk::Update { path, move_path, chunks } => PatchHunk {
                r#type: "update".to_owned(),
                path,
                move_path,
                contents: String::new(),
                chunks,
            },
        })
        .collect()
}

/// `toolTitle` (tool.ts:160-163): shell titles use the command.
fn tool_title(tool_name: &str, input: &Map<String, Value>, fallback: Option<&str>) -> String {
    // (canonicalName is matched through to_tool_kind's table below)
    if is_shell(tool_name) {
        string_value(input.get("command"))
            .or_else(|| string_value(input.get("cmd")))
            .or_else(|| fallback.map(str::to_owned))
            .unwrap_or_else(|| tool_name.to_owned())
    } else {
        fallback.unwrap_or(tool_name).to_owned()
    }
}

/// `rawInput` (tool.ts:165-168): shell input gains the cwd when absent.
fn raw_input(tool_name: &str, input: &Map<String, Value>, cwd: &str) -> Value {
    if !is_shell(tool_name) || input.contains_key("cwd") || input.contains_key("workdir") {
        Value::Object(input.clone())
    } else {
        let mut with_cwd = input.clone();
        with_cwd.insert("cwd".to_owned(), Value::String(cwd.to_owned()));
        Value::Object(with_cwd)
    }
}

fn is_shell(tool_name: &str) -> bool {
    canonical_name(tool_name) == "shell"
}

/// `pendingToolCall` (tool.ts:61-75) shaped as the `ToolCallUpdate` the
/// permission request carries, plus the `ToolCall` the session update
/// stream needs. v2 builds a `ToolCall`; ACP's current schema carries a
/// `ToolCallUpdate` in `session/request_permission`, so both builders
/// share this shaping.
fn base_fields(
    tool_name: &str,
    input: &Map<String, Value>,
    title: Option<&str>,
    cwd: &str,
) -> ToolCallUpdateFields {
    ToolCallUpdateFields::new()
        .title(tool_title(tool_name, input, title))
        .kind(to_tool_kind(tool_name))
        .status(ToolCallStatus::Pending)
        .locations(to_locations(tool_name, input, cwd))
        .raw_input(raw_input(tool_name, input, cwd))
}

/// `pendingToolCall` (tool.ts:61-75) as a session-update `ToolCall`.
#[allow(clippy::needless_pass_by_value)]
#[must_use]
pub fn pending_tool_call(
    tool_call_id: &str,
    tool_name: &str,
    input: &Map<String, Value>,
    title: Option<&str>,
    cwd: &str,
    meta: Option<serde_json::Map<String, Value>>,
) -> ToolCall {
    let fields = base_fields(tool_name, input, title, cwd);
    ToolCall::new(
        tool_call_id.to_owned(),
        fields.title.clone().unwrap_or_else(|| tool_name.to_owned()),
    )
    .name(tool_name.to_owned())
    .kind(fields.kind.unwrap_or(ToolKind::Other))
    .status(ToolCallStatus::Pending)
    .locations(fields.locations.clone().unwrap_or_default())
    .raw_input(fields.raw_input.clone())
    .meta(meta)
}

/// How a permission request's pending update carries `rawInput`
/// (permission.ts:63): the built value when a tracked tool exists,
/// omitted otherwise.
#[derive(Debug, Clone)]
pub enum RawInputOverride {
    Keep,
    Omit,
}

/// The pending shape for `session/request_permission` (permission.ts:53-58):
/// a `ToolCallUpdate` with `status: pending`.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn pending_tool_update(
    tool_call_id: &str,
    tool_name: &str,
    input: &Map<String, Value>,
    title: Option<&str>,
    cwd: &str,
    meta: Option<serde_json::Map<String, Value>>,
    raw_input_override: &RawInputOverride,
    locations_override: Option<Vec<ToolCallLocation>>,
    content: Vec<ToolCallContent>,
) -> ToolCallUpdate {
    let mut fields = base_fields(tool_name, input, title, cwd);
    match raw_input_override {
        RawInputOverride::Keep => {}
        RawInputOverride::Omit => {
            fields.raw_input = None;
        }
    }
    if locations_override.is_some() {
        fields.locations = locations_override;
    }
    if !content.is_empty() {
        fields.content = Some(content);
    }
    ToolCallUpdate::new(tool_call_id.to_owned(), fields).meta(meta)
}

/// `runningToolUpdate` (tool.ts:77-91).
#[must_use]
pub fn running_tool_update(
    tool_call_id: &str,
    tool_name: &str,
    input: &Map<String, Value>,
    cwd: &str,
    meta: Option<serde_json::Map<String, Value>>,
) -> ToolCallUpdate {
    let fields = ToolCallUpdateFields::new()
        .title(tool_title(tool_name, input, None))
        .kind(to_tool_kind(tool_name))
        .status(ToolCallStatus::InProgress)
        .locations(to_locations(tool_name, input, cwd))
        .raw_input(raw_input(tool_name, input, cwd));
    ToolCallUpdate::new(tool_call_id.to_owned(), fields).meta(meta)
}

/// The tool content wire type (`Tool.Content` — text or a data-URI).
pub type ToolContent = Value;

/// `toolContent` (tool.ts:151-158): text parts become text content,
/// `data:image/*;base64` URIs become image content, anything else is
/// dropped.
#[must_use]
pub fn tool_content(content: &[ToolContent]) -> Vec<ToolCallContent> {
    content
        .iter()
        .filter_map(|part| {
            let text = part.get("text").and_then(Value::as_str);
            if let Some(text) = text {
                return Some(ToolCallContent::Content(ToolWrap::new(ContentBlock::Text(
                    TextContent::new(text),
                ))));
            }
            let uri = part.get("uri").and_then(Value::as_str)?;
            let mime = data_uri_image(uri)?;
            let data = data_uri_base64(uri)?;
            Some(ToolCallContent::Content(ToolWrap::new(ContentBlock::Image(
                agent_client_protocol::schema::v1::ImageContent::new(data, mime),
            ))))
        })
        .collect()
}

fn data_uri_image(uri: &str) -> Option<String> {
    // ^data:([^;,]+)(?:;[^,]*)*;base64,(.*)$ (tool.ts:154).
    let rest = uri.strip_prefix("data:")?;
    let (mime, _tail) = rest.split_once(";base64,")?;
    if !mime.starts_with("image/") || mime.contains(',') {
        return None;
    }
    Some(mime.to_owned())
}

fn data_uri_base64(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("data:")?;
    let (_, tail) = rest.split_once(";base64,")?;
    Some(tail.to_owned())
}

/// `completedToolUpdate` (tool.ts:93-125).
#[must_use]
pub fn completed_tool_update(
    tool_call_id: &str,
    tool_name: &str,
    input: &Map<String, Value>,
    content: &[ToolContent],
    metadata: Option<&Map<String, Value>>,
    cwd: &str,
) -> ToolCallUpdate {
    let normalized = tool_content(content);
    let lacks_images = !normalized.iter().any(|part| {
        matches!(
            part,
            ToolCallContent::Content(wrapped) if matches!(wrapped.content, ContentBlock::Image(_))
        )
    });
    // `readDisplayText` strips the line-number gutter the read tool adds;
    // the Rust port renders read output without a gutter, so the text is
    // used as-is (readDisplayText is TUI display shaping, tool.ts:103).
    let images: Vec<ToolCallContent> = if lacks_images {
        Vec::new()
    } else {
        normalized
            .iter()
            .filter(|part| {
                matches!(
                    part,
                    ToolCallContent::Content(wrapped) if matches!(wrapped.content, ContentBlock::Image(_))
                )
            })
            .cloned()
            .collect()
    };
    let primary: Vec<ToolCallContent> = if lacks_images {
        normalized
    } else {
        normalized
            .into_iter()
            .filter(|part| {
                !matches!(
                    part,
                    ToolCallContent::Content(wrapped) if matches!(wrapped.content, ContentBlock::Image(_))
                )
            })
            .collect()
    };
    // Edit-style diff content (tool.ts:112-115).
    let old_text = string_value(input.get("oldString"));
    let new_text = string_value(input.get("newString"));
    let path = file_path(input);
    let mut all = primary;
    if let (Some(old_text), Some(new_text), Some(path)) = (old_text, new_text, path) {
        all.push(ToolCallContent::Diff(
            Diff::new(absolute_path(&path, cwd), new_text).old_text(old_text),
        ));
    }
    all.extend(images);

    let mut raw_output = Map::new();
    if let Some(metadata) = metadata {
        raw_output.insert("metadata".to_owned(), Value::Object(metadata.clone()));
    }
    let fields = ToolCallUpdateFields::new()
        .status(ToolCallStatus::Completed)
        .locations(to_locations(tool_name, input, cwd))
        .content(all)
        .raw_output(Value::Object(raw_output));
    ToolCallUpdate::new(tool_call_id.to_owned(), fields)
}

/// `errorToolUpdate` (tool.ts:127-149).
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn error_tool_update(
    tool_call_id: &str,
    tool_name: &str,
    input: &Map<String, Value>,
    content: Option<&[ToolContent]>,
    metadata: Option<&Map<String, Value>>,
    error: &str,
    cwd: &str,
    meta: Option<serde_json::Map<String, Value>>,
) -> ToolCallUpdate {
    let mut all = tool_content(content.unwrap_or_default());
    all.push(ToolCallContent::Content(ToolWrap::new(ContentBlock::Text(TextContent::new(error)))));
    let mut raw_output = Map::new();
    if let Some(metadata) = metadata {
        raw_output.insert("metadata".to_owned(), Value::Object(metadata.clone()));
    }
    raw_output.insert("error".to_owned(), Value::String(error.to_owned()));
    let fields = ToolCallUpdateFields::new()
        .status(ToolCallStatus::Failed)
        .kind(to_tool_kind(tool_name))
        .title(tool_title(tool_name, input, None))
        .locations(to_locations(tool_name, input, cwd))
        .raw_input(raw_input(tool_name, input, cwd))
        .content(all)
        .raw_output(Value::Object(raw_output));
    ToolCallUpdate::new(tool_call_id.to_owned(), fields).meta(meta)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn input(value: &Value) -> Map<String, Value> {
        value.as_object().cloned().expect("object")
    }

    /// The tool-kind table (tool.ts:10-33) — pinned end to end.
    #[test]
    fn tool_kind_mapping_matches_v2() {
        let cases = [
            ("shell", ToolKind::Execute),
            ("bash", ToolKind::Execute),
            ("webfetch", ToolKind::Fetch),
            ("edit", ToolKind::Edit),
            ("patch", ToolKind::Edit),
            ("write", ToolKind::Edit),
            ("apply_patch", ToolKind::Edit),
            ("grep", ToolKind::Search),
            ("glob", ToolKind::Search),
            ("context", ToolKind::Search),
            ("context7_resolve_library_id", ToolKind::Search),
            ("context7_get_library_docs", ToolKind::Search),
            ("read", ToolKind::Read),
            ("subagent", ToolKind::Think),
            ("task", ToolKind::Think),
            ("list", ToolKind::Other),
            ("websearch", ToolKind::Other),
        ];
        for (name, kind) in cases {
            assert_eq!(to_tool_kind(name), kind, "tool {name}");
        }
    }

    #[test]
    fn shell_locations_use_workdir_or_cwd() {
        let locations = to_locations("shell", &input(&json!({"command": "ls"})), "/w");
        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].path, PathBuf::from("/w"));
        let locations =
            to_locations("shell", &input(&json!({"command": "ls", "workdir": "sub"})), "/w");
        assert_eq!(locations[0].path, PathBuf::from("/w/sub"));
    }

    #[test]
    fn relative_paths_are_made_absolute_against_cwd() {
        let locations = to_locations("read", &input(&json!({"path": "a/b.txt"})), "/w");
        assert_eq!(locations[0].path, PathBuf::from("/w/a/b.txt"));
        // V1 sessions keep `filePath`.
        let locations = to_locations("read", &input(&json!({"filePath": "/abs.txt"})), "/w");
        assert_eq!(locations[0].path, PathBuf::from("/abs.txt"));
    }

    #[test]
    fn patch_locations_cover_targets_and_moves() {
        let patch = "*** Begin Patch\n*** Add File: new.txt\n+hello\n*** Update File: a.txt\n*** Move to: b.txt\n@@\n-x\n+y\n*** End Patch\n";
        let locations = to_locations("patch", &input(&json!({"patchText": patch})), "/w");
        let paths: Vec<&str> = locations.iter().map(|l| l.path.to_str().expect("utf8")).collect();
        assert_eq!(paths, vec!["/w/new.txt", "/w/a.txt", "/w/b.txt"]);
    }

    #[test]
    fn shell_raw_input_gains_cwd_and_title_uses_command() {
        let call = pending_tool_call(
            "tc_1",
            "shell",
            &input(&json!({"command": "cargo test"})),
            None,
            "/w",
            None,
        );
        assert_eq!(call.title, "cargo test");
        assert_eq!(call.raw_input, Some(json!({"command": "cargo test", "cwd": "/w"})));
    }

    #[test]
    fn completed_update_includes_edit_diff_and_images() {
        let content = vec![
            json!({"type": "text", "text": "done"}),
            json!({"type": "file", "uri": "data:image/png;base64,aGk="}),
        ];
        let update = completed_tool_update(
            "tc_2",
            "edit",
            &input(&json!({"path": "f.txt", "oldString": "a", "newString": "b"})),
            &content,
            None,
            "/w",
        );
        let content = update.fields.content.clone().expect("content");
        let ToolCallContent::Diff(diff) = &content[1] else {
            panic!("expected diff at index 1");
        };
        assert_eq!(diff.path, PathBuf::from("/w/f.txt"));
        assert_eq!(diff.old_text.as_deref(), Some("a"));
        assert!(matches!(
            &content[2],
            ToolCallContent::Content(wrapped) if matches!(wrapped.content, ContentBlock::Image(_))
        ));
    }

    #[test]
    fn error_update_carries_the_error_text_and_output() {
        let update = error_tool_update(
            "tc_3",
            "read",
            &input(&json!({"path": "x"})),
            None,
            None,
            "file not found",
            "/w",
            None,
        );
        assert_eq!(update.fields.status, Some(ToolCallStatus::Failed));
        assert_eq!(update.fields.raw_output, Some(json!({ "error": "file not found" })));
    }

    #[test]
    fn non_image_data_uris_are_dropped() {
        let content = vec![json!({"type": "file", "uri": "data:text/plain;base64,aGk="})];
        assert_eq!(tool_content(&content).len(), 0);
    }
}
