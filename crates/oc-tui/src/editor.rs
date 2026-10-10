//! The external `$EDITOR` flow — the port of v2's
//! `packages/tui/src/editor.ts` (`openEditor`).
//!
//! v2 semantics (verified against the reference):
//!
//! * The editor is `VISUAL || EDITOR`; **there is no fallback list** —
//!   unset means the command is a no-op (`openEditor` returns
//!   `undefined`). (The task brief guessed a vi/nano fallback list;
//!   v2.0.24 has none, and the port mirrors that.)
//! * The value goes to `${TMPDIR}/<epoch-ms>.md`, the renderer
//!   **suspends**, the editor runs with inherited stdio (split on
//!   spaces, so `EDITOR="code -w"` works), exit code `0` reads the
//!   file back (empty → `None`), and **any non-zero exit keeps the
//!   old draft** (the error propagates). The temp file is removed and
//!   the renderer resumed on every path.
//! * The caller (`prompt/index.tsx:558`) replaces the prompt with
//!   `normalizePromptContent(content)` when content is non-empty.
//!
//! The Rust port separates the pure invocation ([`open_editor`],
//! testable with a stub editor script) from the terminal
//! suspend/resume the run loop performs around it.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Why the editor flow produced no replacement draft.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorError {
    /// Neither `VISUAL` nor `EDITOR` is set — a no-op, not a failure
    /// (the draft stays untouched).
    NoEditor,
    /// The editor exited non-zero (`code`/`signal`); the old draft
    /// stays (v2 rejects the promise, the caller keeps the prompt).
    Exit(String),
    /// The editor could not spawn / the file could not be managed.
    Io(String),
}

/// `VISUAL` → `EDITOR` → none.
#[must_use]
pub fn resolve_editor() -> Option<String> {
    std::env::var("VISUAL")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| std::env::var("EDITOR").ok().filter(|value| !value.is_empty()))
}

/// `normalizePromptContent` (`prompt/content.ts`): strip ONE trailing
/// newline (CRLF or LF) only when the remainder has no other
/// newlines.
#[must_use]
pub fn normalize_prompt_content(content: &str) -> String {
    let strip = |suffix: &str| -> Option<String> {
        let body = content.strip_suffix(suffix)?;
        (!body.contains('\n') && !body.contains('\r')).then(|| body.to_owned())
    };
    strip("\r\n").or_else(|| strip("\n")).unwrap_or_else(|| content.to_owned())
}

/// The temp-file name: `${tmpdir()}/${Date.now()}.md` (v2 uses
/// `os.tmpdir()`; the port honors `TMPDIR` through the std resolver).
#[must_use]
fn temp_markdown_path() -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_millis())
        .unwrap_or_default();
    std::env::temp_dir().join(format!("{now}.md"))
}

/// Run the external editor over `value`: write the draft to a temp
/// file, `suspend`, spawn the editor (stdio inherited, split on
/// spaces, run from `cwd` when it exists), `resume` on every path,
/// and on a zero exit read the file back.
///
/// Returns `Ok(Some(text))` for a non-empty edited draft (the caller
/// normalizes + replaces), `Ok(None)` for an empty file or unset
/// editor (keep the old draft), `Err` for a non-zero exit (v2 keeps
/// the old draft) or an I/O failure.
pub fn open_editor(
    value: &str,
    cwd: Option<&std::path::Path>,
    suspend: impl FnOnce(),
    resume: impl FnOnce(),
) -> Result<Option<String>, EditorError> {
    let Some(editor) = resolve_editor() else {
        return Ok(None);
    };
    let file = temp_markdown_path();
    std::fs::write(&file, value)
        .map_err(|error| EditorError::Io(format!("write draft: {error}")))?;
    suspend();
    let result = spawn_editor(&editor, &file, cwd);
    resume();
    // v2 reads inside the try and removes in the finally — content
    // first, cleanup on every path.
    let content = std::fs::read_to_string(&file);
    let _ = std::fs::remove_file(&file);
    let status = result?;
    if !status.success() {
        let code = status.code().map_or_else(|| "signal".to_owned(), |code| format!("code {code}"));
        return Err(EditorError::Exit(format!("Editor exited with {code}")));
    }
    match content {
        Ok(content) if content.is_empty() => Ok(None),
        Ok(content) => Ok(Some(content)),
        Err(error) => Err(EditorError::Io(format!("read draft: {error}"))),
    }
}

/// `spawn(parts[0], [...parts.slice(1), file])` with inherited stdio.
fn spawn_editor(
    editor: &str,
    file: &std::path::Path,
    cwd: Option<&std::path::Path>,
) -> Result<std::process::ExitStatus, EditorError> {
    let mut parts = editor.split(' ').filter(|part| !part.is_empty());
    let program = parts.next().unwrap_or_default();
    if program.is_empty() {
        return Err(EditorError::Io("empty editor command".into()));
    }
    let mut command = std::process::Command::new(program);
    command.args(parts);
    command.arg(file);
    if let Some(cwd) = cwd.filter(|dir| dir.is_dir()) {
        command.current_dir(cwd);
    }
    command.status().map_err(|error| EditorError::Io(format!("spawn {program:?}: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    /// A stub editor script: `$1` is the temp file. Modes: append a
    /// line, exit 3, or do nothing.
    fn stub_editor(tag: &str, body: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oc-tui-editor-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("stub-editor.sh");
        let mut file = std::fs::File::create(&script).unwrap();
        // The shebang script receives the draft path as $1.
        writeln!(file, "#!/bin/sh\n{body}\n", body = body.replace("$DRAFT", "\"$1\"")).unwrap();
        make_executable(&script);
        script
    }

    #[cfg(unix)]
    fn make_executable(path: &std::path::Path) {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).unwrap();
    }
    #[cfg(not(unix))]
    fn make_executable(_path: &std::path::Path) {}

    fn with_env(variables: &[(&str, Option<&str>)], run: impl FnOnce()) {
        // The tests in this module run sequentially (one test fn) so
        // process-global env mutation stays sound.
        let saved: Vec<(&str, Option<std::ffi::OsString>)> =
            variables.iter().map(|(name, _)| (*name, std::env::var_os(name))).collect();
        for (name, value) in variables {
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
        run();
        for (name, value) in saved {
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
    }

    /// The editor lifecycle: VISUAL wins over EDITOR; a zero exit
    /// replaces the draft; empty file → None; non-zero exit → the old
    /// draft stays (Err); unset → no-op; suspend/resume always run in
    /// pairs.
    #[test]
    fn editor_lifecycle_per_v2() {
        with_env(&[("VISUAL", None), ("EDITOR", None)], || {
            // Unset: no-op, no suspend/resume cycle.
            let cycles = std::cell::Cell::new(0u32);
            let result = open_editor(
                "draft",
                None,
                || cycles.set(cycles.get() + 1),
                || cycles.set(cycles.get() + 1),
            );
            assert_eq!(result, Ok(None));
            assert_eq!(cycles.get(), 0, "nothing runs without $EDITOR");

            // Zero exit appending a line: the draft comes back edited.
            let script = stub_editor("ok", r#"printf 'edited\n' >> "$DRAFT""#);
            std::env::set_var("EDITOR", &script);
            let cycles = std::cell::Cell::new(0u32);
            let result = open_editor(
                "original\n",
                None,
                || cycles.set(cycles.get() + 1),
                || cycles.set(cycles.get() + 1),
            );
            assert_eq!(result, Ok(Some("original\nedited\n".to_owned())));
            assert_eq!(cycles.get(), 2, "suspend + resume ran once each");

            // VISUAL takes precedence over EDITOR.
            let visual = stub_editor("visual", r#"printf 'V' > "$DRAFT""#);
            std::env::set_var("VISUAL", &visual);
            std::env::set_var("EDITOR", "definitely-not-a-real-binary");
            let result = open_editor("", None, || {}, || {});
            assert_eq!(result, Ok(Some("V".to_owned())));

            // Empty result file: None (keep-old-draft signal).
            let emptier = stub_editor("empty", r#"rm -f "$DRAFT"; touch "$DRAFT""#);
            std::env::set_var("EDITOR", &emptier);
            std::env::remove_var("VISUAL");
            let result = open_editor("old", None, || {}, || {});
            assert_eq!(result, Ok(None));

            // Non-zero exit: Err — the caller keeps the old draft.
            let failer = stub_editor("fail", "exit 3");
            std::env::set_var("EDITOR", &failer);
            let result = open_editor("old", None, || {}, || {});
            assert_eq!(result, Err(EditorError::Exit("Editor exited with code 3".into())));

            // Resume runs even on failure (the finally block).
            let resumed = std::cell::Cell::new(0u32);
            let _ = open_editor("old", None, || {}, || resumed.set(resumed.get() + 1));
            assert_eq!(resumed.get(), 1);
        });
    }

    /// `normalizePromptContent`: one trailing newline drops only for
    /// single-line results; multi-line keeps its newline.
    #[test]
    fn normalize_strips_one_trailing_newline_for_single_lines() {
        assert_eq!(normalize_prompt_content("hello\n"), "hello");
        assert_eq!(normalize_prompt_content("hello\r\n"), "hello");
        assert_eq!(normalize_prompt_content("a\nb\n"), "a\nb\n", "multi-line keeps it");
        assert_eq!(normalize_prompt_content("plain"), "plain");
        assert_eq!(normalize_prompt_content(""), "");
    }
}
