//! Port of `tool-output.ts` — the model-facing output budget (2000 lines /
//! 50 KiB) with file spill and 7-day retention cleanup.
//!
//! Oversized text is written whole to
//! `<data>/tool-output/tool_<identifier>` and the in-band content is reduced
//! to the fitting prefix plus the exact marker
//! `[showing lines 1-N of M; full output saved to <file>]`; non-text parts
//! (file media) are preserved in place. A hourly cleanup task drops spilled
//! files older than the retention window.

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

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use oc_schema::tool::Content;
use oc_util::determinism::Clock;

/// `MAX_LINES` (tool-output.ts:13).
pub const MAX_LINES: usize = 2_000;
/// `MAX_BYTES` — 50 KiB.
pub const MAX_BYTES: usize = 50 * 1024;
/// `RETENTION` — 7 days in milliseconds.
pub const RETENTION_MS: u64 = 7 * 24 * 60 * 60 * 1_000;
/// `DIRECTORY` — the data-root subdirectory for spilled output.
pub const DIRECTORY: &str = "tool-output";

/// Random base62 alphabet (identifier.ts).
const CHARS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
/// Per-millisecond counter for ascending identifiers (identifier.ts).
static LAST: Mutex<(i64, u64)> = Mutex::new((0, 0));

fn random_suffix(len: usize) -> String {
    let mut bytes = vec![0u8; len];
    // Best-effort entropy from the OS; a deterministic fallback keeps the
    // shape stable in constrained test environments.
    if getrandom(&mut bytes).is_err() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as u64)
            .unwrap_or(0);
        let mut state = nanos | 1;
        for byte in bytes.iter_mut() {
            state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            *byte = (state >> 33) as u8;
        }
    }
    bytes.iter().map(|b| CHARS[(*b as usize) % CHARS.len()] as char).collect()
}

fn getrandom(buf: &mut [u8]) -> std::io::Result<()> {
    std::fs::File::open("/dev/urandom")?.read_exact(buf)
}

/// `fileName` — `tool_` + ascending 12-hex time prefix + 14 random base62
/// chars (`Identifier.create(false, timestamp)`).
#[must_use]
pub fn file_name(timestamp_millis: i64) -> String {
    let mut guard = LAST.lock().unwrap_or_else(|poison| poison.into_inner());
    if timestamp_millis != guard.0 {
        guard.0 = timestamp_millis;
        guard.1 = 0;
    }
    guard.1 += 1;
    let counter = guard.1;
    drop(guard);
    // current = ts * 0x1000 + counter; time = top 48 bits, lowercase hex.
    let current = ((timestamp_millis as i128) << 12) + (counter as i128);
    let top = current & ((1i128 << 48) - 1);
    format!("tool_{top:012x}{}", random_suffix(14))
}

/// Configurable limits (`Editor.configure`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_lines: usize,
    pub max_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self { max_lines: MAX_LINES, max_bytes: MAX_BYTES }
    }
}

/// One tool result entering the budget (the normalize-result shape).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ToolOutputResult {
    pub content: Vec<Content>,
    pub metadata: Option<oc_schema::common::Record>,
}

fn text_of(content: &[Content]) -> String {
    content
        .iter()
        .filter_map(|item| match item {
            Content::Text { text } => Some(text.as_str()),
            Content::File { .. } => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn byte_len(text: &str) -> usize {
    text.len()
}

/// RW-T4: JS `String.prototype.length` semantics — UTF-16 code units
/// (chars outside the BMP count 2). The ref truncation budget
/// (`tool-output.ts:93`) is computed with this unit, not Unicode scalars.
fn utf16_len(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

/// JS `String.prototype.slice(0, units)` analogue, rounded down to whole
/// Rust chars (a lone surrogate is not representable in a Rust `String`).
fn utf16_prefix(text: &str, units: usize) -> String {
    let mut taken = 0usize;
    let mut out = String::new();
    for ch in text.chars() {
        let len = ch.len_utf16();
        if taken + len > units {
            break;
        }
        out.push(ch);
        taken += len;
    }
    out
}

/// The ToolOutput service bound to a spill directory and clock.
pub struct ToolOutput {
    directory: PathBuf,
    limits: Mutex<Limits>,
    clock: Arc<dyn Clock>,
}

impl std::fmt::Debug for ToolOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolOutput")
            .field("directory", &self.directory)
            .field("limits", &self.limits.lock().unwrap_or_else(|p| p.into_inner()).clone())
            .finish_non_exhaustive()
    }
}

impl ToolOutput {
    /// Service writing spills under `<data>/tool-output`.
    #[must_use]
    pub fn new(data_root: &Path, clock: Arc<dyn Clock>) -> Self {
        Self { directory: data_root.join(DIRECTORY), limits: Mutex::new(Limits::default()), clock }
    }

    /// Override the configured limits (`Editor.configure`).
    pub fn configure(&self, limits: Limits) {
        *self.limits.lock().unwrap_or_else(|p| p.into_inner()) = limits;
    }

    /// `truncate` — the budget pipeline. Results already carrying a
    /// `truncated` metadata flag pass through untouched.
    pub fn truncate(&self, result: ToolOutputResult) -> std::io::Result<ToolOutputResult> {
        if result.metadata.as_ref().and_then(|meta| meta.get("truncated")).is_some() {
            return Ok(result);
        }
        let limits = *self.limits.lock().unwrap_or_else(|p| p.into_inner());
        let content = result.content;
        let text = text_of(&content);
        let mut lines: Vec<&str> = text.split('\n').collect();
        if text.ends_with('\n') {
            lines.pop();
        }
        let total_bytes = byte_len(&text);
        if lines.len() <= limits.max_lines && total_bytes <= limits.max_bytes {
            let mut metadata = result.metadata.clone().unwrap_or_default();
            metadata.insert("truncated".into(), serde_json::json!(false));
            return Ok(ToolOutputResult { content, metadata: Some(metadata) });
        }

        let mut kept: Vec<&str> = Vec::new();
        let mut bytes = 0usize;
        for line in lines.iter().take(limits.max_lines) {
            let size = byte_len(line) + usize::from(!kept.is_empty());
            if bytes + size > limits.max_bytes {
                break;
            }
            kept.push(line);
            bytes += size;
        }
        std::fs::create_dir_all(&self.directory)?;
        let file = self.directory.join(file_name(self.clock.unix_ms()));
        std::fs::write(&file, text.as_bytes())?;
        let shown =
            if kept.is_empty() { "0 lines".to_string() } else { format!("lines 1-{}", kept.len()) };
        let marker = format!(
            "[showing {shown} of {}; full output saved to {}]",
            lines.len(),
            file.display()
        );

        let mut bounded: Vec<Content> = Vec::new();
        // RW-T4: ref budget accounting is JS `.length`/`.slice()` — UTF-16
        // code units (tool-output.ts:93 `kept.join("\n").length`), so an
        // astral-plane char (emoji) counts as 2, not 1 scalar.
        let mut remaining = utf16_len(&kept.join("\n"));
        let mut seen_text = false;
        let mut marked = false;
        for item in content {
            match item {
                file_item @ Content::File { .. } => bounded.push(file_item),
                Content::Text { text: item_text } => {
                    if seen_text && remaining > 0 {
                        remaining -= 1;
                    }
                    seen_text = true;
                    let item_len = utf16_len(&item_text);
                    if remaining >= item_len {
                        remaining -= item_len;
                        bounded.push(Content::Text { text: item_text });
                        continue;
                    }
                    if remaining > 0 {
                        // JS `.slice(0, remaining)` cuts on a UTF-16 code-unit
                        // boundary (which can split a surrogate pair); Rust
                        // `String` cannot hold a lone surrogate, so this
                        // rounds down to the enclosing char — the kept-unit
                        // accounting above still matches ref exactly.
                        bounded.push(Content::Text { text: utf16_prefix(&item_text, remaining) });
                    }
                    if !marked {
                        bounded.push(Content::Text { text: marker.clone() });
                        marked = true;
                    }
                    remaining = 0;
                }
            }
        }
        if !marked {
            bounded.push(Content::Text { text: marker });
        }
        let mut metadata = result.metadata.clone().unwrap_or_default();
        metadata.insert("truncated".into(), serde_json::json!(true));
        metadata.insert(
            "outputPath".into(),
            serde_json::json!(file.to_string_lossy().replace('\\', "/")),
        );
        Ok(ToolOutputResult { content: bounded, metadata: Some(metadata) })
    }

    /// `cleanup` — drop spilled files whose modification time predates the
    /// retention cutoff (FileRetention.cleanup).
    pub fn cleanup(&self) {
        let cutoff = u64::try_from(self.clock.unix_ms()).unwrap_or(0).saturating_sub(RETENTION_MS);
        let entries = match std::fs::read_dir(&self.directory) {
            Ok(entries) => entries,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !(name.starts_with("tool_")
                && name.len() >= 16
                && name[5..17].chars().all(|c| c.is_ascii_hexdigit()))
            {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            let Ok(modified) = meta.modified() else { continue };
            let modified_ms =
                modified.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
            if (modified_ms as u64) < cutoff {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

/// Spawns the hourly retention cleanup task (`cleanupLayer` — a scoped fiber
/// in v2; a tokio task whose handle the caller retains and aborts at shutdown).
pub fn spawn_cleanup_task(service: Arc<ToolOutput>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(3_600));
        loop {
            interval.tick().await;
            service.cleanup();
        }
    })
}

/// The process-global spill service handle (`makeLocationNode` caches one
/// instance per process; tests relocate the data root explicitly).
static GLOBAL: OnceLock<Arc<ToolOutput>> = OnceLock::new();

/// The shared service rooted at the given data directory.
#[must_use]
pub fn shared(data_root: &Path, clock: Arc<dyn Clock>) -> Arc<ToolOutput> {
    GLOBAL.get_or_init(|| Arc::new(ToolOutput::new(data_root, clock))).clone()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::too_many_lines)]

    use super::*;
    use oc_util::determinism::FixedClock;
    use std::time::Duration;

    fn service(limits: Limits) -> ToolOutput {
        let dir = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
        let clock = Arc::new(FixedClock::new(std::time::UNIX_EPOCH + Duration::from_secs(1_000)));
        let service = ToolOutput::new(dir.path(), clock);
        service.configure(limits);
        // Leak the tempdir for the duration of the test; the spill files are
        // inside it.
        std::mem::forget(dir);
        service
    }

    fn text(text: &str) -> ToolOutputResult {
        ToolOutputResult { content: vec![Content::Text { text: text.into() }], metadata: None }
    }

    #[test]
    fn tool_output_writes_oversized_text_and_returns_bounded_preview() {
        let service = service(Limits { max_lines: 2, max_bytes: 1_000 });
        let result = service.truncate(text("one\ntwo\nthree")).expect("truncate");
        let metadata = result.metadata.expect("metadata");
        assert_eq!(metadata.get("truncated"), Some(&serde_json::json!(true)));
        let output_path = metadata.get("outputPath").and_then(|v| v.as_str()).expect("outputPath");
        assert!(output_path.contains(DIRECTORY));
        assert_eq!(std::fs::read_to_string(output_path).expect("spill"), "one\ntwo\nthree");
        assert_eq!(
            result.content,
            vec![
                Content::Text { text: "one\ntwo".into() },
                Content::Text {
                    text: format!("[showing lines 1-2 of 3; full output saved to {output_path}]")
                },
            ]
        );
    }

    #[test]
    fn tool_output_reports_lines_shown_under_the_byte_limit() {
        let service = service(Limits { max_lines: 100, max_bytes: 5 });
        let result = service.truncate(text("one\ntwo")).expect("truncate");
        let Content::Text { text: marker } = &result.content[1] else { panic!() };
        assert!(
            marker.starts_with("[showing lines 1-1 of 2; full output saved to ")
                && marker.ends_with(']'),
            "marker was {marker}"
        );
    }

    #[test]
    fn tool_output_preserves_mixed_content_ordering() {
        let service = service(Limits { max_lines: 2, max_bytes: 1_000 });
        let file =
            Content::File { uri: "file:///image.png".into(), mime: "image/png".into(), name: None };
        let result = service
            .truncate(ToolOutputResult {
                content: vec![
                    Content::Text { text: "before".into() },
                    file.clone(),
                    Content::Text { text: "after\nomitted".into() },
                ],
                metadata: None,
            })
            .expect("truncate");
        assert_eq!(result.content.len(), 4);
        assert_eq!(result.content[0], Content::Text { text: "before".into() });
        assert_eq!(result.content[1], file);
        assert_eq!(result.content[2], Content::Text { text: "after".into() });
        let Content::Text { text: marker } = &result.content[3] else { panic!() };
        assert!(marker.starts_with("[showing lines 1-2 of 3; full output saved to "));
    }

    #[test]
    fn tool_output_skips_results_that_report_a_truncation_state() {
        let service = service(Limits { max_lines: 1, max_bytes: 1 });
        let truncated = ToolOutputResult {
            content: vec![Content::Text { text: "one\ntwo".into() }],
            metadata: Some(
                serde_json::from_value(serde_json::json!({ "truncated": true, "source": "tool" }))
                    .unwrap(),
            ),
        };
        assert_eq!(service.truncate(truncated.clone()).expect("truncate"), truncated);
    }

    #[test]
    fn tool_output_marks_results_that_fit_without_changing_content() {
        let service = service(Limits::default());
        let result = service.truncate(text("small")).expect("truncate");
        assert_eq!(
            result,
            ToolOutputResult {
                content: vec![Content::Text { text: "small".into() }],
                metadata: Some(
                    serde_json::from_value(serde_json::json!({"truncated": false})).unwrap()
                ),
            }
        );
    }

    #[test]
    fn tool_output_does_not_count_a_trailing_newline_as_another_line() {
        let service = service(Limits { max_lines: 2, max_bytes: 1_000 });
        let result = service.truncate(text("one\ntwo\n")).expect("truncate");
        assert_eq!(
            result,
            ToolOutputResult {
                content: vec![Content::Text { text: "one\ntwo\n".into() }],
                metadata: Some(
                    serde_json::from_value(serde_json::json!({"truncated": false})).unwrap()
                ),
            }
        );
    }

    #[test]
    fn tool_output_reports_a_trailing_newline_omitted_by_the_byte_limit() {
        let service = service(Limits { max_lines: 2, max_bytes: 3 });
        let result = service.truncate(text("one\n")).expect("truncate");
        assert_eq!(result.content[0], Content::Text { text: "one".into() });
        let Content::Text { text: marker } = &result.content[1] else { panic!() };
        assert!(marker.starts_with("[showing lines 1-1 of 1; full output saved to "));
    }

    /// RW-T4: the truncation budget is UTF-16 code units (ref
    /// `tool-output.ts:93` `kept.join("\n").length`), not Unicode scalars —
    /// an astral-plane char counts 2. Direct unit checks.
    #[test]
    fn tool_output_budget_counts_utf16_code_units() {
        assert_eq!(utf16_len("a"), 1);
        assert_eq!(utf16_len("😀"), 2, "astral-plane emoji is 2 UTF-16 units");
        assert_eq!(utf16_len("a😀b"), 4);
        // JS `slice(0, n)` can strand a lone surrogate when n cuts a pair;
        // a Rust String cannot hold one, so the prefix rounds down to the
        // enclosing char (the kept-unit accounting above still matches ref).
        assert_eq!(utf16_prefix("😀b", 1), "");
        assert_eq!(utf16_prefix("😀b", 2), "😀");
        assert_eq!(utf16_prefix("a😀b", 3), "a😀");
        assert_eq!(utf16_prefix("a😀b", 100), "a😀b");
    }

    /// RW-T4: astral-plane truncation end-to-end — the kept-line budget for
    /// `"😀😀😀\n..."` is 6 UTF-16 units (3 scalars under the old port
    /// accounting); under either unit the cut lands on the same char
    /// boundary here, but the emoji sequence must survive whole.
    #[test]
    fn tool_output_truncates_astral_plane_text_on_char_boundaries() {
        let service = service(Limits { max_lines: 1, max_bytes: 1_000 });
        let result = service.truncate(text("😀😀😀\nsecond line")).expect("truncate");
        let Content::Text { text: kept } = &result.content[0] else { panic!() };
        assert_eq!(kept, "😀😀😀", "6 UTF-16 units of budget keep exactly 3 emoji");
        assert!(kept.chars().all(|c| !c.is_ascii()), "no partial scalars");
        let Content::Text { text: marker } = &result.content[1] else { panic!() };
        assert!(marker.starts_with("[showing lines 1-1 of 2; full output saved to "));
    }

    /// RW-T4: mixed BMP + astral content across multiple items — the
    /// separator deduction and per-item fit checks all run in UTF-16 units
    /// and the cut item keeps exactly its kept-line prefix ("a😀" = 3
    /// units), never splitting the surrogate pair.
    #[test]
    fn tool_output_truncates_mixed_bmp_astral_items_in_utf16_units() {
        let service = service(Limits { max_lines: 1, max_bytes: 1_000 });
        let result = service
            .truncate(ToolOutputResult {
                content: vec![
                    Content::Text { text: "a😀\nomitted tail".into() },
                    Content::Text { text: "trailing item".into() },
                ],
                metadata: None,
            })
            .expect("truncate");
        assert_eq!(result.content[0], Content::Text { text: "a😀".into() });
        let Content::Text { text: marker } = &result.content[1] else { panic!() };
        assert!(marker.starts_with("[showing lines 1-1 of 3; full output saved to "));
        // The trailing item beyond the budget is fully replaced by the marker.
        assert_eq!(result.content.len(), 2);
    }

    #[test]
    fn tool_output_file_names_are_ascending_and_match_the_cleanup_shape() {
        let a = file_name(1_725_000_000_000);
        let b = file_name(1_725_000_000_001);
        assert!(a.starts_with("tool_"));
        assert!(b.starts_with("tool_"));
        assert_eq!(a.len(), 5 + 12 + 14);
        assert!(a[5..17].chars().all(|c| c.is_ascii_hexdigit()));
        assert!(a < b, "ascending identifier order (millisecond ordering)");
        // Same millisecond: the counter keeps ascending.
        let c = file_name(1_725_000_000_001);
        assert!(b < c, "counter advances within a millisecond");
        // At the 48-bit wrap (ts around 2^36 with a 12-bit counter) the
        // hex prefix wraps too — cleanup falls back to mtime (covered by
        // the retention test).
        let wrapped = file_name(2_i64.pow(36) + 1);
        assert!(wrapped.starts_with("tool_000000001"), "wrapped prefix: {wrapped}");
    }

    #[test]
    fn tool_output_uses_file_modification_time_when_ids_wrap() {
        let dir = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
        let now = std::time::SystemTime::now();
        let clock = Arc::new(oc_util::determinism::SystemClock);
        let service = ToolOutput::new(dir.path(), clock);
        let directory = dir.path().join(DIRECTORY);
        std::fs::create_dir_all(&directory).expect("dirs");
        let old = directory.join(file_name(2_i64.pow(36) - 1));
        let recent = directory.join(file_name(2_i64.pow(36) + 1));
        std::fs::write(&old, "old").expect("write");
        std::fs::write(&recent, "recent").expect("write");
        set_mtime(&old, now - Duration::from_secs(8 * 24 * 60 * 60));
        set_mtime(&recent, now - Duration::from_secs(60));
        service.cleanup();
        assert!(!old.exists(), "stale spill removed");
        assert!(recent.exists(), "recent spill kept");
    }

    fn set_mtime(path: &Path, at: std::time::SystemTime) {
        let file = std::fs::File::options().write(true).open(path).expect("open for times");
        file.set_times(std::fs::FileTimes::new().set_modified(at)).expect("set mtime");
    }

    /// RA-04: the spawned sweep performs its first pass immediately
    /// (tokio's first interval tick) — boot does not wait an hour before
    /// the disk starts shrinking.
    #[tokio::test]
    async fn tool_output_spawn_cleanup_task_sweeps_on_the_first_tick() {
        let dir = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
        let now = std::time::SystemTime::now();
        let service =
            Arc::new(ToolOutput::new(dir.path(), Arc::new(oc_util::determinism::SystemClock)));
        let directory = dir.path().join(DIRECTORY);
        std::fs::create_dir_all(&directory).expect("dirs");
        let old = directory.join(file_name(1));
        std::fs::write(&old, "old").expect("write");
        set_mtime(&old, now - Duration::from_secs(8 * 24 * 60 * 60));
        let handle = spawn_cleanup_task(Arc::clone(&service));
        for _ in 0..50 {
            tokio::time::sleep(Duration::from_millis(10)).await;
            if !old.exists() {
                break;
            }
        }
        handle.abort();
        assert!(!old.exists(), "spawned sweep removed the stale spill");
    }
}
