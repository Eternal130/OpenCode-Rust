//! Process-backed file defaults — port of
//! `packages/core/src/environment/exec-defaults.ts`.
//!
//! Files derived from spawning processes: one process per intent, `$1` is
//! always the target path. Scripts report classification through an exit-code
//! protocol (44/45/46) so failures never require parsing localized error
//! text; `LC_ALL=C` pins the one stderr match that remains. Requires GNU
//! coreutils and findutils in the target image — BSD and busybox userlands
//! will not work. Malformed output from these scripts is our own bug and
//! fails as `Failed`.

use std::sync::Arc;

use async_trait::async_trait;

use crate::environment::driver::{CommandSpec, ProcessOutput, Spawner};
use crate::environment::files::{
    DirEntry, FileInfo, FileType, Files, FilesError, Range, ReadResult,
};

pub const MAX_DATA_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_ERROR_BYTES: usize = 64 * 1024;
const NOT_FOUND: i32 = 44;
const WRONG_KIND: i32 = 45;
const FAILED: i32 = 46;
const TAB: &str = "\t";

fn load_metadata(flags: &str) -> String {
    format!(
        r#"
metadata=$(stat {flags} -c '%F{TAB}%s{TAB}%Y' -- "$1" 2>&1) || {{
  case "$metadata" in
    *'No such file or directory'*|*'Not a directory'*) exit {NOT_FOUND} ;;
    *) printf '%s' "$metadata" >&2; exit {FAILED} ;;
  esac
}}
"#
    )
}

fn stat_script() -> String {
    format!("{}\nprintf '%s\\n' \"$metadata\"\n", load_metadata(""))
}

fn read_script() -> String {
    format!(
        r#"
{load}
kind=${{metadata%%{TAB}*}}
if [ "$kind" != 'regular file' ] && [ "$kind" != 'regular empty file' ]; then
  printf '%s' "$kind" >&2
  exit {WRONG_KIND}
fi
printf '%s\n' "$metadata"
if [ "$2" = range ]; then
  dd if="$1" iflag=skip_bytes,count_bytes skip="$3" count="$4" status=none
else
  cat -- "$1"
fi
"#,
        load = load_metadata("-L"),
        TAB = TAB,
        WRONG_KIND = WRONG_KIND
    )
}

fn list_script() -> String {
    format!(
        r#"
{load}
kind=${{metadata%%{TAB}*}}
if [ "$kind" != directory ]; then
  printf '%s' "$kind" >&2
  exit {WRONG_KIND}
fi
find -H "$1" -mindepth 1 -maxdepth 1 -printf '%y\0%f\0'
"#,
        load = load_metadata("-L"),
        TAB = TAB,
        WRONG_KIND = WRONG_KIND
    )
}

fn move_script() -> String {
    format!("{load}\nmv -- \"$1\" \"$2\"\n", load = load_metadata(""))
}

const WRITE_SCRIPT: &str = r#"mkdir -p "$(dirname "$1")" && cat > "$1""#;
const REMOVE_SCRIPT: &str = r#"rm -rf -- "$1""#;
const MKDIR_SCRIPT: &str = r#"mkdir -p -- "$1""#;

/// `execDefaults(spawner)`.
pub struct ExecDefaults {
    spawner: Arc<dyn Spawner>,
}

impl ExecDefaults {
    pub fn new(spawner: Arc<dyn Spawner>) -> Self {
        ExecDefaults { spawner }
    }

    async fn run(
        &self,
        path: &str,
        script: &str,
        args: &[String],
        stdin: Option<Vec<u8>>,
    ) -> Result<ProcessOutput, FilesError> {
        let mut env = std::collections::BTreeMap::new();
        env.insert("LC_ALL".to_string(), "C".to_string());
        let mut command_args =
            vec!["-c".to_string(), script.to_string(), "sh".to_string(), path.to_string()];
        command_args.extend(args.iter().cloned());
        let command = CommandSpec {
            command: "sh".to_string(),
            args: command_args,
            env,
            extend_env: true,
            stdin,
            // Collect at most one byte past the ref's stream limits so a
            // runaway process cannot exhaust memory before the post-hoc
            // check below fires (ref collectStream stops AT the limit).
            max_stdout: Some(MAX_DATA_BYTES),
            max_stderr: Some(MAX_ERROR_BYTES),
        };
        let result = self
            .spawner
            .spawn(command)
            .await
            .map_err(|cause| FilesError::failed(path, format!("spawn failed: {cause}")))?;
        if result.stdout.len() > MAX_DATA_BYTES || result.stderr.len() > MAX_ERROR_BYTES {
            return Err(FilesError::failed(path, "Process output exceeded its collection limit"));
        }
        Ok(result)
    }
}

#[async_trait]
impl Files for ExecDefaults {
    async fn stat(&self, path: &str) -> Result<FileInfo, FilesError> {
        let result = self.run(path, &stat_script(), &[], None).await?;
        classify_plain(path, &result, |stdout| parse_info(stdout, path))
    }

    async fn read(&self, path: &str, range: Option<Range>) -> Result<ReadResult, FilesError> {
        let args = match range {
            None => vec!["whole".to_string()],
            Some(range) => {
                vec!["range".to_string(), range.offset.to_string(), range.length.to_string()]
            }
        };
        let result = self.run(path, &read_script(), &args, None).await?;
        classify(path, &result, |stdout| {
            let newline = stdout
                .iter()
                .position(|byte| *byte == b'\n')
                .ok_or_else(|| FilesError::failed(path, "Missing read metadata header"))?;
            Ok(ReadResult {
                info: parse_info(&stdout[..newline], path)?,
                bytes: stdout[newline + 1..].to_vec(),
            })
        })
    }

    async fn write(&self, path: &str, bytes: &[u8]) -> Result<(), FilesError> {
        let result = self.run(path, WRITE_SCRIPT, &[], Some(bytes.to_vec())).await?;
        complete(path, &result)
    }

    async fn list(&self, path: &str) -> Result<Vec<DirEntry>, FilesError> {
        let result = self.run(path, &list_script(), &[], None).await?;
        classify(path, &result, parse_list)
    }

    async fn remove(&self, path: &str) -> Result<(), FilesError> {
        let result = self.run(path, REMOVE_SCRIPT, &[], None).await?;
        complete(path, &result)
    }

    async fn r#move(&self, from: &str, to: &str) -> Result<(), FilesError> {
        let result = self.run(from, &move_script(), &[to.to_string()], None).await?;
        classify_plain(from, &result, |_| Ok(()))
    }

    async fn mkdir(&self, path: &str) -> Result<(), FilesError> {
        let result = self.run(path, MKDIR_SCRIPT, &[], None).await?;
        complete(path, &result)
    }
}

/// `classify` for scripts whose protocol can report `WrongKind`.
fn classify<T>(
    path: &str,
    result: &ProcessOutput,
    success: impl Fn(&[u8]) -> Result<T, FilesError>,
) -> Result<T, FilesError> {
    if result.exit_code == 0 {
        return success(&result.stdout);
    }
    if result.exit_code == NOT_FOUND {
        return Err(FilesError::NotFound { path: path.to_string() });
    }
    if result.exit_code == WRONG_KIND {
        let actual = parse_type(&String::from_utf8_lossy(&result.stderr));
        return Err(FilesError::WrongKind { path: path.to_string(), actual });
    }
    Err(process_failure(path, result))
}

/// `classify` for scripts whose protocol never reports `WrongKind`.
fn classify_plain<T>(
    path: &str,
    result: &ProcessOutput,
    success: impl Fn(&[u8]) -> Result<T, FilesError>,
) -> Result<T, FilesError> {
    if result.exit_code == 0 {
        return success(&result.stdout);
    }
    if result.exit_code == NOT_FOUND {
        return Err(FilesError::NotFound { path: path.to_string() });
    }
    Err(process_failure(path, result))
}

fn complete(path: &str, result: &ProcessOutput) -> Result<(), FilesError> {
    if result.exit_code == 0 {
        Ok(())
    } else {
        Err(process_failure(path, result))
    }
}

fn process_failure(path: &str, result: &ProcessOutput) -> FilesError {
    let stderr = String::from_utf8_lossy(&result.stderr).trim().to_string();
    FilesError::failed(
        path,
        if stderr.is_empty() {
            format!("Process exited with code {}", result.exit_code)
        } else {
            stderr
        },
    )
}

fn parse_info(bytes: &[u8], path: &str) -> Result<FileInfo, FilesError> {
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim();
    let mut parts = text.split(TAB);
    let raw_type = parts.next().unwrap_or_default();
    let raw_size = parts.next().unwrap_or_default();
    let raw_mtime = parts.next().unwrap_or_default();
    let size = raw_size.parse::<u64>();
    let mtime = raw_mtime.parse::<f64>();
    if raw_type.is_empty() {
        // Ref throws a defect here; `path` stays the file path so the error
        // is attributable (the raw stdout rides in `message`).
        return Err(FilesError::failed(path, format!("Invalid stat output: {text}")));
    }
    let size =
        size.map_err(|_| FilesError::failed(path, format!("Invalid stat output: {text}")))?;
    let mtime =
        mtime.map_err(|_| FilesError::failed(path, format!("Invalid stat output: {text}")))?;
    Ok(FileInfo { kind: parse_type(raw_type), size, mtime_ms: mtime * 1_000.0 })
}

fn parse_type(value: &str) -> FileType {
    match value {
        "regular file" | "regular empty file" | "f" => FileType::File,
        "directory" | "d" => FileType::Directory,
        "symbolic link" | "l" => FileType::Symlink,
        _ => FileType::Other,
    }
}

fn parse_list(bytes: &[u8]) -> Result<Vec<DirEntry>, FilesError> {
    let mut fields: Vec<&[u8]> = bytes.split(|byte| *byte == 0).collect();
    fields.pop();
    if !fields.len().is_multiple_of(2) {
        return Err(FilesError::failed("", "Invalid find output"));
    }
    let mut entries = Vec::with_capacity(fields.len() / 2);
    for pair in fields.chunks(2) {
        entries.push(DirEntry {
            name: String::from_utf8_lossy(pair[1]).to_string(),
            kind: parse_type(&String::from_utf8_lossy(pair[0])),
        });
    }
    Ok(entries)
}
