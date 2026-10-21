//! TUI-15 — the Zed editor context (editor-zed.ts:1-278): when the
//! TUI runs INSIDE Zed's terminal (`ZED_TERM=true` or
//! `TERM_PROGRAM=zed`, context/editor.ts:121-123), the composer's
//! footer carries the active editor's selection label
//! (`prompt.footer.file`, prompt/index.tsx:240-250,1945) and a submit
//! attaches the selection as a `<system-reminder>` context block
//! (index.tsx:160-170,1339-1344).
//!
//! The selection reads Zed's OWN sqlite (read-only, never written):
//! `items`/`panes`/`workspaces`/`editors` + `editor_selections`, the
//! active editor's workspace scored against the cwd (the longest
//! containing workspace path wins, ties by recency), and the
//! selection's BYTE offsets mapped to string indices against the
//! editor's buffer contents (falling back to the file on disk).

use std::path::{Path, PathBuf};

/// One selection range (`EditorSelection["ranges"][number]`): the
/// selected text plus its 1-based line/character extent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZedRange {
    pub text: String,
    pub start_line: u32,
    pub start_character: u32,
    pub end_line: u32,
    pub end_character: u32,
}

impl ZedRange {
    /// `hasEditorRangeSelection` (index.tsx:148-153): the range
    /// actually SPANS something (not a bare caret).
    #[must_use]
    pub fn has_selection(&self) -> bool {
        self.start_line != self.end_line || self.start_character != self.end_character
    }

    /// `getEditorRangeLabel` (index.tsx:154-158): `#12` on one line,
    /// `#3-7` across lines — `None` for a bare caret.
    #[must_use]
    pub fn label(&self) -> Option<String> {
        if !self.has_selection() {
            return None;
        }
        if self.start_line == self.end_line {
            Some(format!("#{}", self.start_line))
        } else {
            Some(format!("#{}-{}", self.start_line, self.end_line))
        }
    }
}

/// A resolved editor selection (`EditorSelection`,
/// context/editor.ts:14-31).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZedSelection {
    pub file_path: String,
    pub ranges: Vec<ZedRange>,
}

/// `resolveZedSelection`'s tri-state (editor-zed.ts:34-36).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZedSelectionResult {
    Selection(ZedSelection),
    /// The active editor resolved but carries nothing to attach (no
    /// buffer, no selection) — the label stays hidden.
    Empty,
    /// The db is unreadable/undecodable — the poll keeps trying, the
    /// LAST selection stays (transient failures never clear).
    Unavailable,
}

/// Whether the Zed integration is live (`zedTerminal`,
/// context/editor.ts:121-123): `ZED_TERM=true` or the terminal
/// program is Zed (case-insensitive).
#[must_use]
pub fn zed_terminal_active(env_zed_term: Option<&str>, env_term_program: Option<&str>) -> bool {
    env_zed_term == Some("true")
        || env_term_program.is_some_and(|program| program.to_lowercase() == "zed")
}

/// `resolveZedDbPath` (editor-zed.ts:187-196): `OPENCODE_ZED_DB`
/// first, then the platform defaults — the macOS
/// `~/Library/Application Support/Zed/db/0-stable/db.sqlite` and the
/// Linux `~/.local/share/zed/db/0-stable/db.sqlite`. The first
/// candidate that IS a file wins; `None` when nothing exists.
#[must_use]
pub fn resolve_zed_db_path(
    env_db: Option<&str>,
    home: Option<&str>,
    is_file: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let home = home?;
    let candidates = [
        env_db.map(PathBuf::from),
        Some(PathBuf::from(home).join("Library/Application Support/Zed/db/0-stable/db.sqlite")),
        Some(PathBuf::from(home).join(".local/share/zed/db/0-stable/db.sqlite")),
    ];
    candidates.into_iter().flatten().find(|candidate| is_file(candidate))
}

/// `resolveZedSelection` (editor-zed.ts:38-94): the tri-state over
/// Zed's sqlite — the ACTIVE editor row (workspace-scored against
/// `cwd`), its selections (byte offsets normalized + sorted), and the
/// buffer contents (db row → the file on disk).
// The tri-state's fold reads clearest as a match over divergent arms
// (Empty and Unavailable return DIFFERENT tri-states — not a
// let-else shape).
#[allow(clippy::manual_let_else)]
#[must_use]
pub fn resolve_zed_selection(db_path: &Path, cwd: &Path) -> ZedSelectionResult {
    let Ok(connection) =
        rusqlite::Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
    else {
        return ZedSelectionResult::Unavailable;
    };
    let row = match query_zed_active_editor(&connection, cwd) {
        ActiveEditor::Row(row) => row,
        ActiveEditor::Empty => return ZedSelectionResult::Empty,
        ActiveEditor::Unavailable => return ZedSelectionResult::Unavailable,
    };
    let Some(buffer_path) = row.buffer_path else {
        return ZedSelectionResult::Empty;
    };
    // The selections: nulls drop, min/max normalize, sort by extent.
    let byte_ranges = match query_zed_selections(&connection, row.editor_id, row.workspace_id) {
        Ok(ranges) => ranges,
        Err(()) => return ZedSelectionResult::Unavailable,
    };
    if byte_ranges.is_empty() {
        return ZedSelectionResult::Unavailable;
    }
    // The contents: the db row, else the file on disk.
    let text = match query_zed_contents(&connection, row.editor_id, row.workspace_id) {
        Ok(Some(contents)) => Some(contents),
        Ok(None) => std::fs::read_to_string(&buffer_path).ok(),
        Err(()) => return ZedSelectionResult::Unavailable,
    };
    let Some(text) = text else {
        return ZedSelectionResult::Unavailable;
    };
    let ranges = byte_ranges
        .into_iter()
        .map(|(start, end)| {
            let start_offset = utf8_byte_offset_to_char_index(&text, start);
            let end_offset = utf8_byte_offset_to_char_index(&text, end);
            let (start_line, start_character) = offset_position(&text, start_offset);
            let (end_line, end_character) = offset_position(&text, end_offset);
            ZedRange {
                text: text
                    .chars()
                    .skip(start_offset)
                    .take(end_offset.saturating_sub(start_offset))
                    .collect(),
                start_line,
                start_character,
                end_line,
                end_character,
            }
        })
        .collect();
    ZedSelectionResult::Selection(ZedSelection { file_path: buffer_path, ranges })
}

/// The active-editor query's row (editor-zed.ts:12-19's schema).
struct ActiveEditorRow {
    editor_id: i64,
    workspace_id: i64,
    buffer_path: Option<String>,
}

/// The active-editor query's tri-state.
enum ActiveEditor {
    Row(ActiveEditorRow),
    Empty,
    Unavailable,
}

/// `queryZedActiveEditor` (editor-zed.ts:96-127): the joined
/// items/panes/workspaces/editors row set — every row whose workspace
/// CONTAINS the cwd scores (the longest containing path), the winner
/// by score then recency; an `Editor` item with an editor id and a
/// buffer path is the row.
fn query_zed_active_editor(connection: &rusqlite::Connection, cwd: &Path) -> ActiveEditor {
    let Ok(mut statement) = connection.prepare(
        "select
          i.kind as item_kind,
          e.item_id as editor_id,
          i.workspace_id as workspace_id,
          w.paths as workspace_paths,
          w.timestamp as timestamp,
          e.buffer_path as buffer_path
        from items i
        join panes p on p.pane_id = i.pane_id and p.workspace_id = i.workspace_id
        join workspaces w on w.workspace_id = i.workspace_id
        left join editors e on e.item_id = i.item_id and e.workspace_id = i.workspace_id
        where i.active = 1 and p.active = 1
        order by w.timestamp desc",
    ) else {
        return ActiveEditor::Unavailable;
    };
    let Ok(rows) = statement.query_map([], |row| {
        Ok(ZedEditorRow {
            item_kind: row.get("item_kind")?,
            editor_id: row.get("editor_id")?,
            workspace_id: row.get("workspace_id")?,
            workspace_paths: row.get("workspace_paths")?,
            timestamp: row.get("timestamp")?,
            buffer_path: row.get("buffer_path")?,
        })
    }) else {
        return ActiveEditor::Unavailable;
    };
    let mut parsed: Vec<ZedEditorRow> = Vec::new();
    let mut decode_failed = false;
    for row in rows {
        match row {
            Ok(value) => parsed.push(value),
            Err(_) => decode_failed = true,
        }
    }
    // `raw.length > 0 && rows.length === 0` — rows existed but none
    // decoded: unavailable (editor-zed.ts:116-117).
    if parsed.is_empty() {
        return if decode_failed { ActiveEditor::Unavailable } else { ActiveEditor::Empty };
    }
    let best = parsed
        .iter()
        .map(|row| (row, score_zed_workspace(row.workspace_paths.as_deref(), cwd)))
        .filter(|(_, score)| *score > 0)
        .max_by_key(|(row, score)| (*score, row.timestamp.clone()))
        .map(|(row, _)| row);
    let Some(best) = best else {
        return ActiveEditor::Empty;
    };
    if best.item_kind != "Editor" {
        return ActiveEditor::Unavailable;
    }
    let Some(editor_id) = best.editor_id else {
        return ActiveEditor::Empty;
    };
    ActiveEditor::Row(ActiveEditorRow {
        editor_id,
        workspace_id: best.workspace_id,
        buffer_path: best.buffer_path.clone(),
    })
}

/// The active-editor row's columns (decode failures become
/// [`ActiveEditor::Unavailable`], matching the schema decode).
struct ZedEditorRow {
    item_kind: String,
    editor_id: Option<i64>,
    workspace_id: i64,
    workspace_paths: Option<String>,
    timestamp: String,
    buffer_path: Option<String>,
}

/// `queryZedEditorSelections` (editor-zed.ts:129-158): the editor's
/// selection byte extents — `(min, max)` pairs sorted by extent; null
/// coordinates drop. `Err` = the query failed or none decoded.
fn query_zed_selections(
    connection: &rusqlite::Connection,
    editor_id: i64,
    workspace_id: i64,
) -> Result<Vec<(usize, usize)>, ()> {
    let mut statement = connection
        .prepare(
            "select
              start as selection_start,
              end as selection_end
            from editor_selections
            where editor_id = ?1 and workspace_id = ?2",
        )
        .map_err(|_| ())?;
    let rows = statement
        .query_map([editor_id, workspace_id], |row| {
            Ok((
                row.get::<_, Option<i64>>("selection_start")?,
                row.get::<_, Option<i64>>("selection_end")?,
            ))
        })
        .map_err(|_| ())?;
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for row in rows {
        let Ok((start, end)) = row else { return Err(()) };
        let (Some(start), Some(end)) = (start, end) else { continue };
        let (start, end) =
            (usize::try_from(start.max(0)).unwrap_or(0), usize::try_from(end.max(0)).unwrap_or(0));
        ranges.push((start.min(end), start.max(end)));
    }
    ranges.sort_unstable();
    Ok(ranges)
}

/// `queryZedEditorContents` (editor-zed.ts:160-181): the editor's
/// buffer contents row. `Ok(None)` = the row exists but `contents` is
/// null (the file fallback); `Err` = query/decode failure.
fn query_zed_contents(
    connection: &rusqlite::Connection,
    editor_id: i64,
    workspace_id: i64,
) -> Result<Option<String>, ()> {
    let mut statement = connection
        .prepare("select contents from editors where item_id = ?1 and workspace_id = ?2")
        .map_err(|_| ())?;
    statement
        .query_row([editor_id, workspace_id], |row| row.get::<_, Option<String>>("contents"))
        .map_err(|_| ())
}

/// `scoreZedWorkspace` (editor-zed.ts:199-206): the LONGEST workspace
/// path (resolved) that contains the cwd — 0 when none does.
fn score_zed_workspace(workspace_paths: Option<&str>, cwd: &Path) -> usize {
    zed_workspace_paths(workspace_paths)
        .iter()
        .filter(|item| path_contains(Path::new(item), cwd))
        .map(|item| resolve_path(item).len())
        .max()
        .unwrap_or(0)
}

/// `zedWorkspacePaths` (editor-zed.ts:208-213): a JSON string array,
/// else newline-separated lines.
fn zed_workspace_paths(value: Option<&str>) -> Vec<String> {
    let Some(value) = value else { return Vec::new() };
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(value) {
        if let Some(items) = parsed.as_array() {
            return items.iter().filter_map(|item| item.as_str().map(str::to_owned)).collect();
        }
    }
    value
        .split('\n')
        .map(|line| line.trim_end_matches('\r'))
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// `pathContains` (editor-zed.ts:266-269): `child` equals or sits
/// under `parent` (both resolved lexically — a `..` escape or an
/// absolute jump fails).
fn path_contains(parent: &Path, child: &Path) -> bool {
    resolve_path_buf(child).strip_prefix(resolve_path_buf(parent)).is_ok()
}

/// Absolute, `.`/`..`-component-free path (path.resolve's lexical
/// equivalent — canonicalize only helps for existing trees).
fn resolve_path_buf(value: &Path) -> PathBuf {
    let absolute = if value.is_absolute() {
        value.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")).join(value)
    };
    let mut cleaned = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                cleaned.pop();
            }
            other => cleaned.push(other),
        }
    }
    cleaned
}

fn resolve_path(value: &str) -> String {
    resolve_path_buf(Path::new(value)).display().to_string()
}

/// `utf8ByteOffsetToStringIndex` (editor-zed.ts:215-232): a BYTE
/// offset into the string's CHAR index (the first cluster boundary at
/// or past the byte count — v2's `nextIndex` advances one CHAR per
/// step), clamped to the ends.
fn utf8_byte_offset_to_char_index(text: &str, byte_offset: usize) -> usize {
    if byte_offset == 0 {
        return 0;
    }
    let mut bytes = 0usize;
    let mut chars = 0usize;
    for ch in text.chars() {
        bytes += ch.len_utf8();
        chars += 1;
        if bytes >= byte_offset {
            return chars;
        }
    }
    text.chars().count()
}

/// `offsetsToSelection`'s positions (editor-zed.ts:234-257): 1-based
/// `(line, character)` for a char offset.
fn offset_position(text: &str, offset: usize) -> (u32, u32) {
    let chars: Vec<char> = text.chars().collect();
    let offset = offset.min(chars.len());
    let mut line = 1u32;
    let mut line_start = 0usize;
    for (index, ch) in chars.iter().enumerate().take(offset + 1) {
        if index == offset {
            return (line, u32::try_from(offset - line_start + 1).unwrap_or(u32::MAX));
        }
        if *ch == '\n' {
            line += 1;
            line_start = index + 1;
        }
    }
    (line, u32::try_from(offset - line_start + 1).unwrap_or(u32::MAX))
}

/// `editorFileLabel` (prompt/index.tsx:240-248): the file's basename
/// (`index.*` keeps its parent: `components/index.rs`), plus the
/// first range's label and `+N` for the rest.
#[must_use]
pub fn editor_file_label(selection: &ZedSelection) -> String {
    let path = Path::new(&selection.file_path);
    let filename =
        path.file_name().map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let file = if is_index_file(&filename) {
        let parent = path
            .parent()
            .and_then(|dir| dir.file_name())
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        format!("{parent}/{filename}")
    } else {
        filename
    };
    let range_label = selection.ranges.first().and_then(ZedRange::label).map(|label| {
        if selection.ranges.len() > 1 {
            format!("{label}+{}", selection.ranges.len() - 1)
        } else {
            label
        }
    });
    match range_label {
        Some(label) => format!("{file} {label}"),
        None => file,
    }
}

/// `/^index\.[^./]+$/` — a bare `index.<ext>` basename.
fn is_index_file(filename: &str) -> bool {
    let Some(rest) = filename.strip_prefix("index.") else { return false };
    !rest.is_empty() && !rest.contains('.') && !rest.contains('/')
}

/// `formatEditorContext` (prompt/index.tsx:160-170): the submit's
/// `<system-reminder>` block — the whole-file note when no range
/// spans, else each range's `Selection N: #lines "text"` note.
#[must_use]
pub fn format_editor_context(selection: &ZedSelection) -> String {
    let selected: Vec<&ZedRange> =
        selection.ranges.iter().filter(|range| range.has_selection()).collect();
    if selected.is_empty() {
        return format!(
            "<system-reminder>Note: The user opened the file \"{}\". This may or may not be relevant to the current task.</system-reminder>\n",
            selection.file_path
        );
    }
    let ranges = selected
        .iter()
        .enumerate()
        .map(|(index, range)| {
            let prefix = if selected.len() > 1 {
                format!("Selection {}: ", index + 1)
            } else {
                String::new()
            };
            format!(
                "Note: The user selected {prefix}{} from \"{}\". ```{}```\n\n",
                range.label().unwrap_or_default(),
                selection.file_path,
                range.text
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("<system-reminder>{ranges} This may or may not be relevant to the current task.</system-reminder>\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_offsets_map_to_char_indices() {
        // "héllo": h(1) é(2) l(1) l(1) o(1) — byte 3 is inside é,
        // mapping PAST it to char 2.
        let text = "héllo";
        assert_eq!(utf8_byte_offset_to_char_index(text, 0), 0);
        assert_eq!(utf8_byte_offset_to_char_index(text, 1), 1);
        assert_eq!(utf8_byte_offset_to_char_index(text, 3), 2);
        assert_eq!(utf8_byte_offset_to_char_index(text, 6), 5);
        // Past the end clamps to the length.
        assert_eq!(utf8_byte_offset_to_char_index(text, 999), 5);
    }

    #[test]
    fn positions_are_one_based() {
        let text = "ab\ncd";
        assert_eq!(offset_position(text, 0), (1, 1));
        assert_eq!(offset_position(text, 2), (1, 3));
        assert_eq!(offset_position(text, 3), (2, 1));
        assert_eq!(offset_position(text, 5), (2, 3));
    }

    #[test]
    fn workspace_paths_parse_json_then_lines() {
        assert_eq!(
            zed_workspace_paths(Some(r#"["/a","/b"]"#)),
            vec!["/a".to_owned(), "/b".to_owned()]
        );
        assert_eq!(zed_workspace_paths(Some("/x\n/y")), vec!["/x".to_owned(), "/y".to_owned()]);
        assert_eq!(zed_workspace_paths(None), Vec::<String>::new());
    }

    #[test]
    fn db_path_prefers_env_then_platform() {
        // The "file exists" predicate only accepts the env override
        // and the LINUX platform default — the macOS candidate never
        // resolves on this box.
        let exists = |path: &Path| {
            path.ends_with("custom.sqlite")
                || path.ends_with(".local/share/zed/db/0-stable/db.sqlite")
        };
        let env = resolve_zed_db_path(Some("/zed/custom.sqlite"), Some("/home/u"), exists);
        assert_eq!(env, Some(PathBuf::from("/zed/custom.sqlite")));
        let linux = resolve_zed_db_path(None, Some("/home/u"), exists);
        assert_eq!(linux, Some(PathBuf::from("/home/u/.local/share/zed/db/0-stable/db.sqlite")));
        let none = resolve_zed_db_path(None, None, exists);
        assert_eq!(none, None);
    }

    #[test]
    fn file_label_uses_basename_and_index_parent() {
        let selection = ZedSelection {
            file_path: "/proj/src/main.rs".to_owned(),
            ranges: vec![ZedRange {
                text: String::new(),
                start_line: 3,
                start_character: 1,
                end_line: 7,
                end_character: 2,
            }],
        };
        assert_eq!(editor_file_label(&selection), "main.rs #3-7");
        let multi = ZedSelection {
            file_path: "/proj/src/components/index.tsx".to_owned(),
            ranges: vec![
                ZedRange {
                    text: String::new(),
                    start_line: 2,
                    start_character: 1,
                    end_line: 2,
                    end_character: 5,
                },
                ZedRange {
                    text: String::new(),
                    start_line: 9,
                    start_character: 1,
                    end_line: 9,
                    end_character: 4,
                },
                ZedRange {
                    text: String::new(),
                    start_line: 11,
                    start_character: 1,
                    end_line: 11,
                    end_character: 3,
                },
            ],
        };
        assert_eq!(editor_file_label(&multi), "components/index.tsx #2+2");
    }

    #[test]
    fn editor_context_formats_open_note_and_selections() {
        let open = ZedSelection { file_path: "/a.rs".to_owned(), ranges: Vec::new() };
        assert!(format_editor_context(&open)
            .starts_with("<system-reminder>Note: The user opened the file \"/a.rs\"."));
        let selected = ZedSelection {
            file_path: "/a.rs".to_owned(),
            ranges: vec![ZedRange {
                text: "let x = 1;".to_owned(),
                start_line: 4,
                start_character: 1,
                end_line: 4,
                end_character: 11,
            }],
        };
        let formatted = format_editor_context(&selected);
        assert!(formatted.contains("Note: The user selected #4 from \"/a.rs\""), "{formatted}");
        assert!(formatted.contains("```let x = 1;```"), "{formatted}");
    }

    #[test]
    fn zed_terminal_gate_matches_env() {
        assert!(zed_terminal_active(Some("true"), None));
        assert!(zed_terminal_active(None, Some("zed")));
        assert!(zed_terminal_active(None, Some("Zed")));
        assert!(!zed_terminal_active(Some("false"), Some("xterm")));
    }

    /// The full resolver against a REAL sqlite fixture (a temp-file
    /// db with Zed's table shapes): one workspace containing the cwd,
    /// an active editor with a multi-byte selection and a reversed
    /// second extent.
    #[test]
    fn resolver_reads_a_zed_shaped_db() {
        let dir = std::env::temp_dir().join(format!("oc-zed-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let db_path = dir.join("db.sqlite");
        let connection = rusqlite::Connection::open(&db_path).expect("open");
        connection
            .execute_batch(&format!(
                "create table items (item_id integer primary key, kind text, pane_id integer, workspace_id integer, active integer);
                 create table panes (pane_id integer, workspace_id integer, active integer);
                 create table workspaces (workspace_id integer primary key, paths text, timestamp text);
                 create table editors (item_id integer, workspace_id integer, buffer_path text, contents text);
                 create table editor_selections (editor_id integer, workspace_id integer, start integer, end integer);
                 insert into workspaces values (1, '[\"{0}\"]', '2026-01-02T00:00:00Z');
                 insert into panes values (10, 1, 1);
                 insert into items values (100, 'Editor', 10, 1, 1);
                 insert into editors values (100, 1, '{0}/notes.md', 'héllo wörld');
                 insert into editor_selections values (100, 1, 0, 6);
                 insert into editor_selections values (100, 1, 8, 7);",
                dir.display()
            ))
            .expect("schema");
        drop(connection);
        let result = resolve_zed_selection(&db_path, &dir);
        let ZedSelectionResult::Selection(selection) = result else {
            panic!("expected a selection, got {result:?}");
        };
        assert!(selection.file_path.ends_with("notes.md"));
        assert_eq!(selection.ranges.len(), 2);
        // Range 1: bytes 0..6 of "héllo wörld" = "héllo".
        assert_eq!(selection.ranges[0].text, "héllo");
        assert_eq!(selection.ranges[0].start_line, 1);
        assert_eq!(selection.ranges[0].end_line, 1);
        // Range 2: the reversed pair normalized to bytes 7..8 = "w".
        assert_eq!(selection.ranges[1].text, "w");
        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_dir(&dir);
    }
}
