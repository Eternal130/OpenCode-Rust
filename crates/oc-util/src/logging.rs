//! Shared logging: the runtime log-level knob (`OPENCODE_LOG_LEVEL`,
//! v2 `util/src/observability/logging.ts:164-173`) and the shared log
//! file sink + in-place trim (`logging.ts:59-157`), hoisted from
//! oc-server's `errors.rs` so every crate (oc-core, oc-server) can
//! route warn/debug lines through the same gate.

use std::path::{Path, PathBuf};

/// Log levels, ordered `Debug < Info < Warn < Error`
/// (`effect/LogLevel`; `logging.ts:166-171`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    /// The level's label as it appears in a log line.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Level::Debug => "DEBUG",
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
        }
    }
}

/// `minimumLogLevel()` (`logging.ts:164-173`):
/// `OPENCODE_LOG_LEVEL?.toUpperCase()` mapped over
/// {DEBUG,INFO,WARN,ERROR}; invalid or absent → `Info`.
#[must_use]
pub fn parse_level(value: Option<&str>) -> Level {
    let Some(value) = value else {
        return Level::Info;
    };
    match value.to_uppercase().as_str() {
        "DEBUG" => Level::Debug,
        "INFO" => Level::Info,
        "WARN" => Level::Warn,
        "ERROR" => Level::Error,
        _ => Level::Info,
    }
}

/// The process-wide floor, parsed once at first use (v2 evaluates the
/// env when the logging layer is constructed at boot; a `OnceLock` is
/// the Rust equivalent — the env is not re-read per line).
pub fn minimum_log_level() -> Level {
    static MINIMUM: std::sync::OnceLock<Level> = std::sync::OnceLock::new();
    *MINIMUM.get_or_init(|| parse_level(std::env::var("OPENCODE_LOG_LEVEL").ok().as_deref()))
}

/// Whether a line at `level` passes the [`minimum_log_level`] floor.
#[must_use]
pub fn should_emit(level: Level) -> bool {
    level >= minimum_log_level()
}

/// The stderr gate (`logging.ts:159-162`): the stderr copy of every
/// accepted line is a per-message `OPENCODE_PRINT_LOGS === "1"` check.
#[must_use]
pub fn stderr_enabled() -> bool {
    std::env::var("OPENCODE_PRINT_LOGS").as_deref() == Ok("1")
}

/// The routed warn/info line (G-387): every line that passes the
/// floor is ALWAYS appended to the shared log file
/// ([`append_server_log`]) and copied to stderr only when
/// `OPENCODE_PRINT_LOGS === "1"` (checked per call, mirroring v2's
/// `stderrLogger`).
pub fn log(level: Level, line: &str) {
    if !should_emit(level) {
        return;
    }
    append_server_log(&format!("[{}] {}", level.label(), line));
    if stderr_enabled() {
        eprintln!("[{}] {}", level.label(), line);
    }
}

/// Appends one line to the server log sink
/// (`OPENCODE_LOG_FILE`, else v2's layout `<data>/log/opencode.log` —
/// see [`server_log_path`]), prefixed with a unix-ms timestamp, then
/// runs the in-place trim check ([`trim_log`]). Failures are silent —
/// logging must never take a request down — and the file is opened per
/// line (error rates are low; no lifecycle to manage across forked
/// daemons).
pub fn append_server_log(line: &str) {
    use std::io::Write as _;
    let path = server_log_path();
    if let Some(parent) = path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_millis());
        let _ = writeln!(file, "[ts {ms}] {line}");
    }
    // v2's file logger trims on a schedule; the per-line sink checks on
    // append (a stat per logged line — the sink only sees errors and
    // lifecycle transitions).
    let _ = trim_log(&path);
}

/// The log sink location: `OPENCODE_LOG_FILE` when set (tests and
/// embedders), else v2's layout (`util/src/observability/logging.ts:59-61`
/// via `loggers(app.channel === "local", app.channel)`,
/// `observability.ts:31`): the shared `data/log/opencode.log` for every
/// release channel, `data/log/opencode-<sanitized>.log` for the `local`
/// dev channel.
#[must_use]
pub fn server_log_path() -> PathBuf {
    if let Some(path) = std::env::var_os("OPENCODE_LOG_FILE").filter(|v| !v.is_empty()) {
        return PathBuf::from(path);
    }
    let channel = crate::channel::current();
    crate::global_roots::resolve().log().join(log_filename(&channel))
}

/// `Logging.file(local, channel)` (`logging.ts:59-62`): the global
/// `opencode.log` unless the process runs the `local` dev channel
/// (`local = app.channel === "local"`), which gets the channel-suffixed
/// `opencode-<sanitized>.log` (channel chars outside
/// `[a-zA-Z0-9._-]` become `-`).
#[must_use]
pub fn log_filename(channel: &str) -> String {
    if channel == "local" {
        let sanitized: String =
            channel
                .chars()
                .map(|ch| {
                    if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
                        ch
                    } else {
                        '-'
                    }
                })
                .collect();
        format!("opencode-{sanitized}.log")
    } else {
        "opencode.log".to_owned()
    }
}

/// v2's log bounds (`logging.ts:9-10`): compact in place once the file
/// passes 50 MiB, keeping ~25 MiB (rounded forward to the next line
/// boundary).
const LOG_MAX_BYTES: u64 = 50 * 1024 * 1024;
const LOG_KEEP_BYTES: u64 = 25 * 1024 * 1024;

/// A trim lock older than this belongs to a dead process
/// (`logging.ts:13`).
const LOG_TRIM_LOCK_STALE: std::time::Duration = std::time::Duration::from_secs(300);

/// The trim copy chunk (`logging.ts:14`).
const LOG_TRIM_CHUNK: usize = 64 * 1024;

/// `Logging.trim` (`logging.ts:86-124`): compact the log IN PLACE — the
/// head is dropped so roughly `keep` bytes remain, starting at the
/// first line boundary at or past `size - keep`. In-place (never
/// rename) because other processes hold the file open `O_APPEND` and a
/// rename would strand them on the unlinked inode. The mkdir-based
/// `<file>.trim` lock serializes concurrent trimmers; a stale lock
/// (> 5 min) is broken but still yields this round. Best-effort: every
/// failure is silently skipped (the next append retries).
fn trim_log(path: &Path) -> std::io::Result<()> {
    trim_log_options(path, LOG_MAX_BYTES, LOG_KEEP_BYTES)
}

/// The trim, with explicit bounds (tests).
pub fn trim_log_options(path: &Path, max: u64, keep: u64) -> std::io::Result<()> {
    // mkdir as the atomic lock primitive (`create_dir` fails when the
    // directory exists).
    let mut lock = std::ffi::OsString::from(path.as_os_str());
    lock.push(".trim");
    let lock = PathBuf::from(lock);
    match std::fs::create_dir(&lock) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let stale = std::fs::metadata(&lock)
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|modified| {
                    modified.elapsed().ok().filter(|age| *age > LOG_TRIM_LOCK_STALE)
                })
                .is_some();
            if stale {
                let _ = std::fs::remove_dir_all(&lock);
            }
            return Ok(()); // another trimmer owns it (or broke our lock)
        }
        Err(error) => return Err(error),
    }
    let result = trim_locked(path, max, keep);
    let _ = std::fs::remove_dir_all(&lock);
    result
}

fn trim_locked(path: &Path, max: u64, keep: u64) -> std::io::Result<()> {
    use std::io::{Read, Seek, SeekFrom, Write};

    let size = std::fs::metadata(path)?.len();
    if size <= max {
        return Ok(());
    }
    let mut probe = std::fs::OpenOptions::new().read(true).open(path)?;
    let start = line_start(&mut probe, size.saturating_sub(keep))?;
    drop(probe);
    // Read from `start` to EOF (lines appended since the stat survive);
    // the write cursor always trails the read cursor, so the forward
    // copy never overwrites unread bytes. Separate opens (not dup'd
    // descriptors) so the cursors move independently.
    let mut reader = std::fs::File::open(path)?;
    reader.seek(SeekFrom::Start(start))?;
    let mut writer = std::fs::OpenOptions::new().write(true).open(path)?;
    writer.seek(SeekFrom::Start(0))?;
    let mut written = 0u64;
    let mut buffer = vec![0u8; LOG_TRIM_CHUNK];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        writer.write_all(&buffer[..read])?;
        written += read as u64;
    }
    writer.flush()?;
    writer.set_len(written)?;
    Ok(())
}

/// The first byte after the first newline at or beyond `from`
/// (`logging.ts:145-157`), or EOF when the tail has no newline.
fn line_start(file: &mut std::fs::File, from: u64) -> std::io::Result<u64> {
    use std::io::{Read, Seek, SeekFrom};
    let mut cursor = from;
    let mut buffer = vec![0u8; LOG_TRIM_CHUNK];
    loop {
        file.seek(SeekFrom::Start(cursor))?;
        let read = file.read(&mut buffer)?;
        if read == 0 {
            return Ok(cursor);
        }
        let Some(newline) = buffer[..read].iter().position(|byte| *byte == b'\n') else {
            cursor += read as u64;
            continue;
        };
        return Ok(cursor + newline as u64 + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializes env mutation for the log-sink tests (one process,
    /// many parallel tests).
    static LOG_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// `minimumLogLevel` (`logging.ts:164-173`): case-insensitive
    /// {DEBUG,INFO,WARN,ERROR}; invalid garbage and absent → Info.
    #[test]
    fn level_parse_matrix() {
        assert_eq!(parse_level(Some("debug")), Level::Debug);
        assert_eq!(parse_level(Some("DEBUG")), Level::Debug);
        assert_eq!(parse_level(Some("Debug")), Level::Debug);
        assert_eq!(parse_level(Some("info")), Level::Info);
        assert_eq!(parse_level(Some("warn")), Level::Warn);
        assert_eq!(parse_level(Some("error")), Level::Error);
        assert_eq!(parse_level(Some("garbage")), Level::Info);
        assert_eq!(parse_level(Some("")), Level::Info);
        assert_eq!(parse_level(None), Level::Info);
        // Ordering: Debug < Info < Warn < Error.
        assert!(Level::Debug < Level::Info);
        assert!(Level::Info < Level::Warn);
        assert!(Level::Warn < Level::Error);
    }

    /// The default floor (no env) is Info: DEBUG lines are suppressed,
    /// WARN lines pass.
    #[test]
    fn default_floor_is_info() {
        let _guard = LOG_ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let saved = std::env::var_os("OPENCODE_LOG_LEVEL");
        std::env::remove_var("OPENCODE_LOG_LEVEL");
        assert_eq!(minimum_log_level_for_test(), Level::Info);
        assert!(!should_emit(Level::Debug));
        assert!(should_emit(Level::Info));
        assert!(should_emit(Level::Warn));
        assert!(should_emit(Level::Error));
        match saved {
            Some(value) => std::env::set_var("OPENCODE_LOG_LEVEL", value),
            None => std::env::remove_var("OPENCODE_LOG_LEVEL"),
        }
    }

    /// Re-parse the floor from the CURRENT env (the OnceLock caches the
    /// boot value; tests exercise the parse directly).
    fn minimum_log_level_for_test() -> Level {
        parse_level(std::env::var("OPENCODE_LOG_LEVEL").ok().as_deref())
    }

    /// `log()`: an accepted line ALWAYS lands in the log file
    /// (`OPENCODE_LOG_FILE` override), whether or not the stderr copy
    /// is on; a DEBUG line at the default Info floor writes nothing.
    #[test]
    fn log_routes_to_file_with_floor_and_stderr_gate() {
        let _guard = LOG_ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let dir = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        let sink = dir.path().join("opencode.log");
        let saved_log = std::env::var_os("OPENCODE_LOG_FILE");
        let saved_print = std::env::var_os("OPENCODE_PRINT_LOGS");
        std::env::set_var("OPENCODE_LOG_FILE", &sink);
        std::env::remove_var("OPENCODE_PRINT_LOGS");

        // Accepted warn: file always; stderr gate off (still files).
        log(Level::Warn, "[probe] warn line");
        // Suppressed debug at the default Info floor: nothing written.
        log(Level::Debug, "[probe] debug line");
        // The stderr gate is a per-message check (`logging.ts:160`).
        assert!(!stderr_enabled());
        std::env::set_var("OPENCODE_PRINT_LOGS", "1");
        assert!(stderr_enabled());
        log(Level::Error, "[probe] error line");

        std::env::remove_var("OPENCODE_PRINT_LOGS");
        match saved_print {
            Some(value) => std::env::set_var("OPENCODE_PRINT_LOGS", value),
            None => std::env::remove_var("OPENCODE_PRINT_LOGS"),
        }
        match saved_log {
            Some(value) => std::env::set_var("OPENCODE_LOG_FILE", value),
            None => std::env::remove_var("OPENCODE_LOG_FILE"),
        }

        let contents = std::fs::read_to_string(&sink).expect("sink written");
        assert!(contents.contains("[WARN] [probe] warn line"), "{contents}");
        assert!(contents.contains("[ERROR] [probe] error line"), "{contents}");
        assert!(!contents.contains("debug line"), "floor suppression: {contents}");
        assert!(contents.contains("[ts "), "unix-ms timestamp prefix present");
    }

    /// v2's log-file naming (`logging.ts:59-62` via `loggers(local =
    /// channel === "local", channel)`, `observability.ts:31`): the
    /// shared `opencode.log` for every release channel; the `local` dev
    /// channel gets `opencode-<sanitized>.log`.
    #[test]
    fn log_file_name_follows_the_v2_layout() {
        let _guard = LOG_ENV_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let saved_channel = std::env::var_os("OPENCODE_CHANNEL");
        let saved_log = std::env::var_os("OPENCODE_LOG_FILE");
        let saved = [
            ("XDG_DATA_HOME", std::env::var_os("XDG_DATA_HOME")),
            ("XDG_CONFIG_HOME", std::env::var_os("XDG_CONFIG_HOME")),
            ("XDG_STATE_HOME", std::env::var_os("XDG_STATE_HOME")),
            ("XDG_CACHE_HOME", std::env::var_os("XDG_CACHE_HOME")),
        ];
        let sandbox = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        for (name, _) in &saved {
            std::env::remove_var(name);
        }
        std::env::set_var("OPENCODE_LOG_FILE", "");
        std::env::set_var("XDG_DATA_HOME", sandbox.path().join("data"));

        let restore = || {
            for (name, value) in &saved {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
            match saved_channel.as_ref() {
                Some(value) => std::env::set_var("OPENCODE_CHANNEL", value),
                None => std::env::remove_var("OPENCODE_CHANNEL"),
            }
            match saved_log.as_ref() {
                Some(value) => std::env::set_var("OPENCODE_LOG_FILE", value),
                None => std::env::remove_var("OPENCODE_LOG_FILE"),
            }
        };

        // Release channels (incl. the `latest` default): the shared file.
        std::env::remove_var("OPENCODE_CHANNEL");
        let path = server_log_path();
        assert_eq!(
            path,
            sandbox.path().join("data/opencode-rust/log/opencode.log"),
            "the default channel logs to v2's shared opencode.log"
        );
        std::env::set_var("OPENCODE_CHANNEL", "stable");
        assert_eq!(
            server_log_path(),
            sandbox.path().join("data/opencode-rust/log/opencode.log"),
            "any non-local channel shares the global file"
        );
        // The local dev channel: the channel-suffixed file.
        std::env::set_var("OPENCODE_CHANNEL", "local");
        assert_eq!(
            server_log_path(),
            sandbox.path().join("data/opencode-rust/log/opencode-local.log")
        );
        restore();

        // The named unit: `file(local, channel)`.
        assert_eq!(log_filename("local"), "opencode-local.log");
        assert_eq!(log_filename("latest"), "opencode.log");
    }

    /// `Logging.trim` (`logging.ts:86-157`): past the max the file
    /// compacts IN PLACE to ~keep bytes, cutting at a line boundary;
    /// under the max it is untouched; the `.trim` lock is removed.
    #[test]
    fn log_trim_compacts_in_place_at_a_line_boundary() {
        let dir = tempfile::tempdir_in("/tmp/opencode").expect("sandbox");
        let log = dir.path().join("opencode.log");
        // 10 lines of 10 bytes; max 50 (trims), keep 20 → keeps the
        // newest ~2-3 lines starting at a line boundary.
        let lines: Vec<String> = (0..10).map(|index| format!("line{index:04}\n")).collect();
        let body: String = lines.concat();
        std::fs::write(&log, &body).expect("seed");
        let total = body.len() as u64;

        // Under the max: untouched.
        trim_log_options(&log, total + 1, 20).expect("no-op trim");
        assert_eq!(std::fs::read_to_string(&log).unwrap(), body);

        // Over the max: the head drops, the tail survives whole lines.
        trim_log_options(&log, total - 1, 20).expect("trim");
        let trimmed = std::fs::read_to_string(&log).unwrap();
        assert!(trimmed.len() <= 30, "roughly keep bytes remain: {trimmed}");
        assert!(trimmed.ends_with("line0009\n"), "the newest line survives");
        assert!(trimmed.starts_with("line"), "the cut lands on a line boundary: {trimmed:?}");
        assert!(
            body.ends_with(&trimmed),
            "the kept bytes are the file's tail (in-place compaction)"
        );
        assert!(!log.with_file_name("opencode.log.trim").exists(), "lock cleaned up");

        // An existing lock defers the trim (another trimmer owns it).
        std::fs::write(&log, &body).expect("reseed");
        std::fs::create_dir(log.with_file_name("opencode.log.trim")).expect("lock");
        trim_log_options(&log, total - 1, 20).expect("deferred");
        assert_eq!(std::fs::read_to_string(&log).unwrap(), body, "not trimmed");
        std::fs::remove_dir(log.with_file_name("opencode.log.trim")).expect("unlock");
    }
}
