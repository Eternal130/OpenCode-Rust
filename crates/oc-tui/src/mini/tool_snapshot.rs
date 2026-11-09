//! G-134 — the mini completed-state tool structured snapshots (ref
//! `mini/tool.ts`, 1358 lines: `snapWrite`/`snapEdit`/`snapPatch`/
//! `snapTask` + `toolView`'s snap table).
//!
//! Bounded scope (G-134): the user-visible essentials of the
//! completed Write/Edit/Patch/Task tools — the structured CARD (title
//! row + body: written content, per-file diffs, task rows) — ported
//! onto the mono scrollback's row model. Out of architecture scope
//! (documented bounded deviations, G-134):
//!
//! * the ref's `toolScroll` live-output window and the interactive
//!   scrollable expansion render through opentui `DiffRenderable` /
//!   `CodeRenderable` virtualization; the mono writer prints rows
//!   into the terminal's own scrollback (already scrollable);
//! * the code-block syntax highlight paints through the theme
//!   elsewhere in the port — the mono card is plain rows;
//! * `snapQuestion`/`snapForm` and the LSP/bash inline families stay
//!   caption-only (not part of the gap's named subset).

use serde_json::Value;

/// One diff card item (`ToolSnapshot["diff"]["items"]`, tool.ts:
/// 552-562 / 578-593).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DiffItem {
    pub title: String,
    pub diff: String,
    pub file: String,
    pub deletions: u64,
}

/// The structured snapshot (`ToolSnapshot`, tool.ts:60-100): the
/// completed tool's card body.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ToolSnapshot {
    /// `kind: "code"` (`snapWrite`, tool.ts:529-543).
    Code { title: String, content: String, file: String },
    /// `kind: "diff"` (`snapEdit`/`snapPatch`).
    Diff { items: Vec<DiffItem> },
    /// `kind: "task"` (`snapTask`, tool.ts:601-612).
    Task { title: String, rows: Vec<String> },
    /// No structured snapshot (the caption subset stays).
    #[default]
    None,
}

/// `list<T>()` (tool.ts:122-126): a JSON array field, tolerating
/// non-array garbage.
fn list(value: Option<&Value>) -> &[Value] {
    value.and_then(Value::as_array).map_or(&[] as &[Value], Vec::as_slice)
}

fn str_field(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or_default().to_owned()
}

/// The display path: the basename half of the caption subset (the
/// ref's `toolPath` abbreviates under the frame's directory; the mono
/// card keeps the input path verbatim — bounded, G-134).
fn display_path(file: &str) -> String {
    file.to_owned()
}

/// `snapWrite` (tool.ts:529-543): `# Wrote {path}` + the written
/// content.
#[must_use]
pub fn snap_write(input: &Value) -> ToolSnapshot {
    let file = str_field(input, "path");
    let content = str_field(input, "content");
    if file.is_empty() && content.is_empty() {
        return ToolSnapshot::None;
    }
    ToolSnapshot::Code { title: format!("# Wrote {}", display_path(&file)), content, file }
}

/// `snapEdit` (tool.ts:544-562): `# Edited {path}` + the first
/// metadata file's patch (the raw input `diff` fallback).
#[must_use]
pub fn snap_edit(input: &Value, metadata: &Value) -> ToolSnapshot {
    let item = list(metadata.get("files")).first().cloned().unwrap_or(Value::Null);
    let file = item
        .get("file")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .filter(|file| !file.is_empty())
        .unwrap_or_else(|| str_field(input, "path"));
    let diff = item
        .get("patch")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .filter(|patch| !patch.is_empty())
        .unwrap_or_else(|| str_field(metadata, "diff"));
    if file.is_empty() || diff.trim().is_empty() {
        return ToolSnapshot::None;
    }
    ToolSnapshot::Diff {
        items: vec![DiffItem {
            title: format!("# Edited {}", display_path(&file)),
            diff,
            file,
            deletions: 0,
        }],
    }
}

/// `patchTitle` (tool.ts:513-527): `# Created/Deleted/Moved/Patched`.
#[must_use]
pub fn patch_title(file: &Value) -> String {
    let status = file.get("status").and_then(Value::as_str).unwrap_or_default();
    let name = file.get("file").and_then(Value::as_str).unwrap_or_default();
    match status {
        "added" => format!("# Created {}", display_path(name)),
        "deleted" => format!("# Deleted {}", display_path(name)),
        "moved" => {
            let from = file.get("from").and_then(Value::as_str).unwrap_or_default();
            format!("# Moved {} -> {}", display_path(from), display_path(name))
        }
        _ => format!("# Patched {}", display_path(name)),
    }
}

/// `snapPatch` (tool.ts:564-599): one diff card item per patched file
/// (empty patches drop the whole snapshot — the ref's all-or-nothing
/// `items.length !== files.length` guard).
#[must_use]
pub fn snap_patch(meta: &Value) -> ToolSnapshot {
    let files = list(meta.get("files"));
    if files.is_empty() {
        return ToolSnapshot::None;
    }
    let mut items = Vec::new();
    for file in files {
        let Value::Object(_) = file else { return ToolSnapshot::None };
        let diff = file.get("patch").and_then(Value::as_str).unwrap_or_default();
        if diff.trim().is_empty() {
            return ToolSnapshot::None;
        }
        items.push(DiffItem {
            title: patch_title(file),
            diff: diff.to_owned(),
            file: str_field(file, "file"),
            deletions: file.get("deletions").and_then(Value::as_u64).unwrap_or(0),
        });
    }
    ToolSnapshot::Diff { items }
}

/// `snapTask` (tool.ts:601-612): `# {Kind} Subagent` + the
/// description rows.
#[must_use]
pub fn snap_task(input: &Value) -> ToolSnapshot {
    let agent = str_field(input, "agent");
    let kind = title_case(if agent.is_empty() { "general" } else { &agent });
    let description = str_field(input, "description");
    ToolSnapshot::Task {
        title: format!("# {kind} Subagent"),
        rows: if description.is_empty() { Vec::new() } else { vec![description] },
    }
}

/// `Locale.titlecase` (the mono subset: first letter upper).
fn title_case(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// `toolView(name).snap` (tool.ts:920-967's table): the completed
/// tool's structured snapshot, or [`ToolSnapshot::None`] for the
/// caption-only families.
#[must_use]
pub fn tool_snapshot(tool: &str, input: &Value, metadata: &Value, meta: &Value) -> ToolSnapshot {
    match tool {
        "write" => snap_write(input),
        "edit" | "multiedit" => snap_edit(input, metadata),
        "patch" | "apply_patch" => snap_patch(meta),
        "task" | "subagent" => snap_task(input),
        _ => ToolSnapshot::None,
    }
}

/// The mono card rows (`structuredBody`'s row projection, tool.ts:
/// 1290-1339 — bounded: plain rows, no syntax/diff renderable):
/// the title, then the body lines. Empty diffs/cards render nothing
/// beyond the caption.
#[must_use]
pub fn snapshot_rows(snapshot: &ToolSnapshot) -> Vec<String> {
    let mut rows = Vec::new();
    match snapshot {
        ToolSnapshot::None => {}
        ToolSnapshot::Code { title, content, .. } => {
            rows.push(title.clone());
            rows.extend(content.split('\n').map(str::to_owned));
        }
        ToolSnapshot::Diff { items } => {
            for item in items {
                rows.push(item.title.clone());
                rows.extend(item.diff.split('\n').map(str::to_owned));
            }
        }
        ToolSnapshot::Task { title, rows: task_rows } => {
            rows.push(title.clone());
            rows.extend(task_rows.iter().cloned());
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn write_snapshots_the_content_card() {
        let input = json!({"path": "src/main.rs", "content": "fn main() {}\n"});
        let snapshot = tool_snapshot("write", &input, &Value::Null, &Value::Null);
        assert_eq!(
            snapshot,
            ToolSnapshot::Code {
                title: "# Wrote src/main.rs".to_owned(),
                content: "fn main() {}\n".to_owned(),
                file: "src/main.rs".to_owned(),
            }
        );
        assert_eq!(snapshot_rows(&snapshot), ["# Wrote src/main.rs", "fn main() {}", ""]);
        // No path AND no content: no snapshot (tool.ts:533-535).
        assert_eq!(
            tool_snapshot("write", &json!({}), &Value::Null, &Value::Null),
            ToolSnapshot::None
        );
    }

    #[test]
    fn edit_snapshots_the_first_metadata_file() {
        let input = json!({"path": "a.txt"});
        let metadata = json!({"files": [{"file": "b.txt", "patch": "@@ -1 +1 @@\n-a\n+b\n"}]});
        let snapshot = tool_snapshot("edit", &input, &metadata, &Value::Null);
        let ToolSnapshot::Diff { items } = snapshot else {
            panic!("expected a diff card");
        };
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "# Edited b.txt");
        assert!(items[0].diff.starts_with("@@ -1 +1 @@"));
        // The raw-diff fallback.
        let metadata = json!({"diff": "@@ -1 +1 @@\n-a\n+b\n"});
        let snapshot = tool_snapshot("edit", &input, &metadata, &Value::Null);
        assert!(matches!(snapshot, ToolSnapshot::Diff { .. }));
        // No diff at all: none.
        assert_eq!(tool_snapshot("edit", &input, &json!({}), &Value::Null), ToolSnapshot::None);
    }

    #[test]
    fn patch_snapshots_one_card_per_file() {
        let meta = json!({"files": [
            {"file": "new.txt", "status": "added", "patch": "--- a\n+++ b\n+hello\n", "deletions": 0},
            {"file": "old.txt", "status": "deleted", "patch": "--- a\n+++ b\n-world\n", "deletions": 1},
            {"file": "x.txt", "status": "moved", "from": "y.txt", "patch": "@@\n", "deletions": 0},
            {"file": "z.txt", "status": "modified", "patch": "@@\n", "deletions": 2},
        ]});
        let snapshot = tool_snapshot("patch", &Value::Null, &Value::Null, &meta);
        let ToolSnapshot::Diff { items } = snapshot else {
            panic!("expected a diff card");
        };
        assert_eq!(items.len(), 4);
        assert_eq!(items[0].title, "# Created new.txt");
        assert_eq!(items[1].title, "# Deleted old.txt");
        assert_eq!(items[1].deletions, 1);
        assert_eq!(items[2].title, "# Moved y.txt -> x.txt");
        assert_eq!(items[3].title, "# Patched z.txt");
        // An empty-patch file drops the whole snapshot (all-or-nothing).
        let meta = json!({"files": [{"file": "a.txt", "patch": "  "}]});
        assert_eq!(tool_snapshot("patch", &Value::Null, &Value::Null, &meta), ToolSnapshot::None);
        assert_eq!(
            tool_snapshot("patch", &Value::Null, &Value::Null, &json!({})),
            ToolSnapshot::None
        );
    }

    #[test]
    fn task_snapshots_the_subagent_card() {
        let input = json!({"agent": "engineer", "description": "fix the bug"});
        let snapshot = tool_snapshot("task", &input, &Value::Null, &Value::Null);
        assert_eq!(
            snapshot,
            ToolSnapshot::Task {
                title: "# Engineer Subagent".to_owned(),
                rows: vec!["fix the bug".to_owned()],
            }
        );
        assert_eq!(snapshot_rows(&snapshot), ["# Engineer Subagent", "fix the bug"]);
        // No agent: "General".
        let snapshot =
            tool_snapshot("task", &json!({"description": "d"}), &Value::Null, &Value::Null);
        assert!(
            matches!(&snapshot, ToolSnapshot::Task { title, .. } if title == "# General Subagent")
        );
    }

    #[test]
    fn caption_families_have_no_snapshot() {
        for tool in ["bash", "read", "grep", "glob", "webfetch", "unknown"] {
            assert_eq!(
                tool_snapshot(tool, &json!({"command": "ls"}), &Value::Null, &Value::Null),
                ToolSnapshot::None
            );
        }
    }
}
