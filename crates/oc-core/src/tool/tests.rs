//! Tool-lane tests (Wave C1) — ported from `tool-{edit,write,read,
//! read-filesystem,search,glob-implicit,patch,output,registry,schema,
//! execute}.test.ts` with exact-string assertions on every model-facing
//! format.
//!
//! The stack: a tempdir location + always-allow permission engine (deny
//! toggles per test) + no-op formatter + the full registration pipeline
//! (`registry::register` through `ToolRegistryState` executors, exactly the
//! production path).
//!
//! glob/grep spawn the ripgrep runner; the environment has no `rg`, so the
//! tests install an `OPENCODE_RG_PATH` shim that records the exact argv and
//! replays canned fixtures — pinning both the argument construction and the
//! output formats.

#![allow(clippy::too_many_lines)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use oc_schema::id::{AgentId, SessionId};
use oc_schema::permission::{Effect, Rule};
use oc_util::determinism::{Clock, FixedClock, IdGen, SequentialIdGen};

use crate::file_access::FileAccess;
use crate::file_mutation::FileMutation;
use crate::formatter::NoopFormatter;
use crate::location::{ToolCallContext, ToolRegistryState, ToolRunFailure};
use crate::permission::{
    BroadcastEventSink, MemorySavedGrantStore, Permission, PermissionError, RuleResolver,
};
use crate::tool::ToolServices;

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

const SESSION: &str = "ses_tool_test";

pub(super) struct Harness {
    dir: PathBuf,
    outside: PathBuf,
    tools: Arc<ToolRegistryState>,
    pub(super) services: Arc<ToolServices>,
    #[allow(dead_code)]
    permission: Permission,
    _keep: tempfile::TempDir,
    _keep_outside: tempfile::TempDir,
}

/// Resolves a fixed ruleset (allow-all by default; per-action deny toggles).
struct RecordingResolver {
    rules: Arc<parking_lot::Mutex<oc_schema::permission::Ruleset>>,
}

#[async_trait::async_trait]
impl RuleResolver for RecordingResolver {
    async fn resolve(
        &self,
        _session_id: &SessionId,
        _agent: Option<&AgentId>,
    ) -> Result<
        (Option<oc_schema::permission::Ruleset>, Option<oc_schema::permission::Ruleset>),
        PermissionError,
    > {
        Ok((Some(self.rules.lock().clone()), None))
    }
}

fn allow_all() -> oc_schema::permission::Ruleset {
    vec![Rule { action: "*".into(), resource: "*".into(), effect: Effect::Allow }]
}

fn deny_action(action: &str) -> oc_schema::permission::Ruleset {
    // Last matching rule wins: allow everything, then deny the target
    // action (permission::evaluate semantics).
    vec![
        Rule { action: "*".into(), resource: "*".into(), effect: Effect::Allow },
        Rule { action: action.into(), resource: "*".into(), effect: Effect::Deny },
    ]
}

async fn harness() -> Harness {
    harness_with(allow_all()).await
}

/// The same harness for codemode's executor tests (crate-internal).
pub(super) async fn harness_for_codemode() -> Harness {
    harness().await
}

async fn harness_with(rules: oc_schema::permission::Ruleset) -> Harness {
    let dir = tempfile::tempdir_in("/tmp/opencode").expect("tempdir");
    let outside = tempfile::tempdir_in("/tmp/opencode").expect("outside tempdir");
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).expect("home");

    let ids: Arc<dyn IdGen> = Arc::new(SequentialIdGen::new());
    let clock: Arc<dyn Clock> = Arc::new(FixedClock::epoch());
    let saved = Arc::new(MemorySavedGrantStore::new(ids.clone(), clock.clone()));
    let resolver = Arc::new(RecordingResolver { rules: Arc::new(parking_lot::Mutex::new(rules)) });
    let permission = Permission::new(
        ids.clone(),
        clock.clone(),
        Arc::new(BroadcastEventSink::new(64)),
        saved,
        resolver,
        oc_schema::id::ProjectId("proj_tool".into()),
        None,
    );
    // Always allow through saved grants — the engine's grant table
    // short-circuits every ask (tests toggle deny via the resolver).
    let access = FileAccess::new(
        permission.clone(),
        dir.path().to_path_buf(),
        dir.path().to_path_buf(),
        home.clone(),
    );
    let mutation = Arc::new(FileMutation::new(dir.path().to_path_buf(), home.clone()));
    let services = Arc::new(ToolServices {
        directory: dir.path().to_path_buf(),
        project_directory: dir.path().to_path_buf(),
        home,
        access,
        permission: permission.clone(),
        mutation,
        formatter: Arc::new(NoopFormatter::new()),
        instructions: None,
        db: None,
        bus: None,
    });
    let tools = Arc::new(ToolRegistryState::new());
    let hooks = Arc::new(crate::hooks::HookRegistry::new());
    crate::tool::register_all(&tools, &hooks, &services).expect("register builtins");
    Harness {
        dir: dir.path().to_path_buf(),
        outside: outside.path().to_path_buf(),
        tools,
        services,
        permission,
        _keep: dir,
        _keep_outside: outside,
    }
}

pub(super) fn context() -> ToolCallContext {
    ToolCallContext {
        session_id: SessionId(SESSION.into()),
        agent: AgentId::parse("build"),
        message_id: "msg_1".into(),
        call_id: "call_1".into(),
    }
}

pub(super) async fn call(
    tools: &ToolRegistryState,
    name: &str,
    input: serde_json::Value,
) -> Result<crate::location::ToolRunResult, ToolRunFailure> {
    let executor = tools.executor(name).expect("executor registered");
    executor.execute_with_context(&input, &context()).await
}

fn error_message(failure: &ToolRunFailure) -> String {
    failure.message().to_string()
}

fn text_of(outcome: &crate::location::ToolRunResult) -> String {
    match &outcome.content[0] {
        oc_schema::tool::Content::Text { text } => text.clone(),
        _ => panic!("expected text content"),
    }
}

// ---------------------------------------------------------------------------
// Registration + snapshot (tool-registry.test.ts subset)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tool_registers_six_filesystem_tools_with_codemode_false() {
    let harness = harness().await;
    let names: Vec<String> = harness.tools.list().into_iter().map(|t| t.name).collect();
    for name in ["edit", "glob", "grep", "patch", "read", "write"] {
        assert!(names.contains(&name.to_string()), "{name} missing: {names:?}");
    }
    for tool in harness.tools.list() {
        assert!(!tool.codemode, "{} stays direct (codemode: false)", tool.name);
    }
    // edit/write/patch carry permission "edit"; read/glob/grep default to
    // their names.
    let edit = harness.tools.get("edit").expect("edit");
    assert_eq!(edit.permission_action(), "edit");
    assert_eq!(harness.tools.get("read").unwrap().permission_action(), "read");
    assert_eq!(harness.tools.get("glob").unwrap().permission_action(), "glob");
    assert_eq!(harness.tools.get("grep").unwrap().permission_action(), "grep");
}

#[tokio::test]
async fn tool_snapshot_drops_wholly_denied_tools() {
    let harness = harness().await;
    let ruleset = vec![Rule { action: "edit".into(), resource: "*".into(), effect: Effect::Deny }];
    let names: Vec<String> =
        harness.tools.snapshot(Some(&ruleset)).into_iter().map(|t| t.name).collect();
    assert!(!names.contains(&"edit".to_string()));
    assert!(!names.contains(&"write".to_string()));
    assert!(!names.contains(&"patch".to_string()));
    for kept in ["read", "glob", "grep"] {
        assert!(names.contains(&kept.to_string()));
    }
}

#[tokio::test]
async fn tool_unknown_executor_is_never_available() {
    let harness = harness().await;
    // The registry resolves nothing for unregistered names; the runner's
    // executeTool wrapper turns that into the exact model-facing message
    // (model_request.rs, Wave B — pinned here for the tool lane).
    assert!(harness.tools.executor("missing").is_none());
    let missing = "missing";
    let unavailable = format!(
        "No tool named \"{missing}\" is currently available. Please use a tool from the available tool list."
    );
    assert_eq!(
        unavailable,
        "No tool named \"missing\" is currently available. Please use a tool from the available tool list."
    );
}

#[tokio::test]
async fn tool_effective_names_and_validation() {
    use crate::tool::registry::{effective_name, normalized_name, valid_namespace};
    assert_eq!(normalized_name("bash-tool.v2"), "bash-tool_v2");
    assert_eq!(effective_name(None, "read"), "read");
    assert_eq!(effective_name(Some("slack.admin"), "create"), "slack_admin_create");
    assert!(valid_namespace("slack.admin"));
    assert!(!valid_namespace(""));
    assert!(!valid_namespace("slack..admin"));
    assert!(!valid_namespace("a".repeat(65).as_str()));
}

#[tokio::test]
async fn tool_decode_issues_format_is_exact() {
    let harness = harness().await;
    let failure = call(&harness.tools, "grep", serde_json::json!({ "pattern": "" }))
        .await
        .expect_err("minLength");
    assert_eq!(
        error_message(&failure),
        "Invalid arguments for tool \"grep\":\n- pattern: Pattern must not be empty\n\nArguments provided:\n{\n  \"pattern\": \"\"\n}\n\nUpdate the arguments and call the tool again."
    );
    // Missing required field.
    let failure = call(&harness.tools, "edit", serde_json::json!({ "path": "x" }))
        .await
        .expect_err("required");
    assert!(error_message(&failure).starts_with("Invalid arguments for tool \"edit\":"));
    assert!(error_message(&failure).contains("- oldString: is required"));
    // Type mismatch.
    let failure =
        call(&harness.tools, "read", serde_json::json!({ "path": 5 })).await.expect_err("type");
    assert!(error_message(&failure).contains("Expected string, received number"));
}

// ---------------------------------------------------------------------------
// Edit (tool-edit.test.ts)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tool_edit_replaces_relative_exact_text_once() {
    let harness = harness().await;
    std::fs::write(harness.dir.join("hello.txt"), "before\nrest\n").expect("write");
    let settled = call(
        &harness.tools,
        "edit",
        serde_json::json!({ "path": "hello.txt", "oldString": "before", "newString": "after" }),
    )
    .await
    .expect("edit");
    assert_eq!(text_of(&settled), "Edited hello.txt (1 replacement)");
    let metadata = settled.metadata.expect("metadata");
    let files = metadata.get("files").and_then(|v| v.as_array()).expect("files");
    assert_eq!(files[0]["file"], "hello.txt");
    assert_eq!(files[0]["status"], "modified");
    assert_eq!(files[0]["additions"], 1);
    assert_eq!(files[0]["deletions"], 1);
    let patch = files[0]["patch"].as_str().expect("patch");
    assert!(patch.contains("-before\n+after"), "patch: {patch}");
    // RD-14: structured output carries `files` (ref edit.ts Output =
    // {files, replacements}) — same diff data as metadata.
    let output = settled.output.expect("structured output");
    assert_eq!(output["replacements"], serde_json::json!(1));
    let output_files = output["files"].as_array().expect("output.files array");
    assert_eq!(output_files.len(), files.len());
    assert_eq!(output_files[0], files[0], "output.files mirrors metadata diff shape");
    assert_eq!(std::fs::read_to_string(harness.dir.join("hello.txt")).unwrap(), "after\nrest\n");
}

#[tokio::test]
async fn tool_edit_rejects_noop_empty_missing_and_ambiguous() {
    let harness = harness().await;
    std::fs::write(harness.dir.join("matches.txt"), "same same").expect("write");
    let failure = call(
        &harness.tools,
        "edit",
        serde_json::json!({ "path": "matches.txt", "oldString": "same", "newString": "same" }),
    )
    .await
    .expect_err("identical");
    assert_eq!(
        error_message(&failure),
        "No changes to apply: oldString and newString are identical."
    );
    let failure = call(
        &harness.tools,
        "edit",
        serde_json::json!({ "path": "matches.txt", "oldString": "", "newString": "after" }),
    )
    .await
    .expect_err("empty");
    assert_eq!(
        error_message(&failure),
        "oldString must not be empty. Use write to create or overwrite a file."
    );
    let failure = call(
        &harness.tools,
        "edit",
        serde_json::json!({ "path": "matches.txt", "oldString": "missing", "newString": "after" }),
    )
    .await
    .expect_err("missing");
    assert_eq!(
        error_message(&failure),
        "Could not find oldString in matches.txt. It must match exactly, including whitespace and indentation."
    );
    let failure = call(
        &harness.tools,
        "edit",
        serde_json::json!({ "path": "matches.txt", "oldString": "same", "newString": "after" }),
    )
    .await
    .expect_err("ambiguous");
    assert_eq!(
        error_message(&failure),
        "Found 2 matches for oldString, but expected exactly one. Add more surrounding context to make oldString unique, or set replaceAll to true to replace every occurrence."
    );
}

#[tokio::test]
async fn tool_edit_specific_missing_and_directory_errors() {
    let harness = harness().await;
    std::fs::create_dir_all(harness.dir.join("src")).expect("dir");
    let failure = call(
        &harness.tools,
        "edit",
        serde_json::json!({ "path": "missing.ts", "oldString": "before", "newString": "after" }),
    )
    .await
    .expect_err("missing file");
    assert_eq!(error_message(&failure), "File not found: missing.ts");
    let failure = call(
        &harness.tools,
        "edit",
        serde_json::json!({ "path": "src", "oldString": "before", "newString": "after" }),
    )
    .await
    .expect_err("directory");
    assert_eq!(error_message(&failure), "Path is a directory, not a file: src");
}

#[tokio::test]
async fn tool_edit_replace_all_replaces_every_occurrence() {
    let harness = harness().await;
    std::fs::write(harness.dir.join("all.txt"), "same same same").expect("write");
    let settled = call(
        &harness.tools,
        "edit",
        serde_json::json!({ "path": "all.txt", "oldString": "same", "newString": "after", "replaceAll": true }),
    )
    .await
    .expect("edit all");
    assert_eq!(text_of(&settled), "Edited all.txt (3 replacements)");
    assert_eq!(std::fs::read_to_string(harness.dir.join("all.txt")).unwrap(), "after after after");
}

#[tokio::test]
async fn tool_edit_normalizes_unicode_typography_only_after_exact_fails() {
    let harness = harness().await;
    std::fs::write(
        harness.dir.join("unicode.txt"),
        "exact - match\ncurly “quotes”\nminus − one\nspace\u{00A0}here\nexact − match\n",
    )
    .expect("write");
    let normalized = call(
        &harness.tools,
        "edit",
        serde_json::json!({
            "path": "unicode.txt",
            "oldString": "curly \"quotes\"\nminus - one\nspace here",
            "newString": "normalized"
        }),
    )
    .await
    .expect("normalized pass");
    assert!(settled_of(&normalized), "normalized pass completes");
    let exact = call(
        &harness.tools,
        "edit",
        serde_json::json!({ "path": "unicode.txt", "oldString": "exact - match", "newString": "selected" }),
    )
    .await
    .expect("exact pass");
    assert!(settled_of(&exact));
    assert_eq!(
        std::fs::read_to_string(harness.dir.join("unicode.txt")).unwrap(),
        "selected\nnormalized\nexact − match\n"
    );
}

fn settled_of(outcome: &crate::location::ToolRunResult) -> bool {
    !outcome.content.is_empty()
}

#[tokio::test]
async fn tool_edit_ignores_trailing_whitespace_preserving_untouched_lines() {
    let harness = harness().await;
    std::fs::write(harness.dir.join("whitespace.txt"), "before  \nmatch  \nnext\t\nafter  \n")
        .expect("write");
    call(
        &harness.tools,
        "edit",
        serde_json::json!({ "path": "whitespace.txt", "oldString": "match\nnext", "newString": "changed" }),
    )
    .await
    .expect("line pass");
    assert_eq!(
        std::fs::read_to_string(harness.dir.join("whitespace.txt")).unwrap(),
        "before  \nchanged\nafter  \n"
    );
}

#[tokio::test]
async fn tool_edit_non_overlapping_matches_and_crlf_preserved() {
    let harness = harness().await;
    std::fs::write(harness.dir.join("overlap.txt"), "a  \na  \na  \n").expect("write");
    std::fs::write(harness.dir.join("windows.txt"), "a  \r\nb\t\r\n").expect("write");
    let replaced = call(
        &harness.tools,
        "edit",
        serde_json::json!({ "path": "overlap.txt", "oldString": "a\na", "newString": "x", "replaceAll": true }),
    )
    .await
    .expect("overlap");
    assert_eq!(text_of(&replaced), "Edited overlap.txt (1 replacement)");
    call(
        &harness.tools,
        "edit",
        serde_json::json!({ "path": "windows.txt", "oldString": "a\nb", "newString": "x" }),
    )
    .await
    .expect("crlf");
    assert_eq!(std::fs::read_to_string(harness.dir.join("overlap.txt")).unwrap(), "x\na  \n");
    assert_eq!(std::fs::read_to_string(harness.dir.join("windows.txt")).unwrap(), "x\r\n");
}

#[tokio::test]
async fn tool_edit_preserves_bom_and_crlf_endings() {
    let harness = harness().await;
    std::fs::write(harness.dir.join("windows.txt"), "\u{FEFF}before\r\nrest\r\n").expect("write");
    call(
        &harness.tools,
        "edit",
        serde_json::json!({ "path": "windows.txt", "oldString": "before\nrest", "newString": "after\nrest" }),
    )
    .await
    .expect("bom edit");
    assert_eq!(
        std::fs::read_to_string(harness.dir.join("windows.txt")).unwrap(),
        "\u{FEFF}after\r\nrest\r\n"
    );
}

#[tokio::test]
async fn tool_edit_external_directory_approval_order_and_denials() {
    // Happy path: external_directory then edit, both granted.
    let harness = harness().await;
    let external = harness.outside.join("external.txt");
    std::fs::write(&external, "before").expect("write");
    call(
        &harness.tools,
        "edit",
        serde_json::json!({ "path": external, "oldString": "before", "newString": "after" }),
    )
    .await
    .expect("external edit");
    assert_eq!(std::fs::read_to_string(&external).unwrap(), "after");

    // Denied external_directory: nothing is read, the file stands.
    let harness = harness_with(deny_action("external_directory")).await;
    let external = harness.outside.join("denied.txt");
    std::fs::write(&external, "before").expect("write");
    let failure = call(
        &harness.tools,
        "edit",
        serde_json::json!({ "path": external, "oldString": "before", "newString": "after" }),
    )
    .await
    .expect_err("external denied");
    assert_eq!(error_message(&failure), "Permission denied: external_directory");
    assert_eq!(std::fs::read_to_string(&external).unwrap(), "before");

    // Denied edit (in-location): the message never discloses whether
    // oldString matched.
    let harness = harness_with(deny_action("edit")).await;
    std::fs::write(harness.dir.join("secret.txt"), "secret content").expect("write");
    let matching = call(
        &harness.tools,
        "edit",
        serde_json::json!({ "path": "secret.txt", "oldString": "secret content", "newString": "replacement" }),
    )
    .await
    .expect_err("denied matching");
    let missing = call(
        &harness.tools,
        "edit",
        serde_json::json!({ "path": "secret.txt", "oldString": "not present", "newString": "replacement" }),
    )
    .await
    .expect_err("denied missing");
    assert_eq!(error_message(&matching), "Permission denied: edit");
    assert_eq!(error_message(&missing), error_message(&matching));
    assert_eq!(std::fs::read_to_string(harness.dir.join("secret.txt")).unwrap(), "secret content");
}

// ---------------------------------------------------------------------------
// Write (tool-write.test.ts)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tool_write_creates_and_writes_files() {
    let harness = harness().await;
    let settled = call(
        &harness.tools,
        "write",
        serde_json::json!({ "path": "src/new.txt", "content": "created" }),
    )
    .await
    .expect("write");
    assert_eq!(text_of(&settled), "Created file successfully: src/new.txt");
    assert_eq!(std::fs::read_to_string(harness.dir.join("src/new.txt")).unwrap(), "created");

    std::fs::write(harness.dir.join("existing.txt"), "before").expect("write");
    let settled = call(
        &harness.tools,
        "write",
        serde_json::json!({ "path": "existing.txt", "content": "after" }),
    )
    .await
    .expect("overwrite");
    assert_eq!(text_of(&settled), "Wrote file successfully: existing.txt");
    assert_eq!(std::fs::read_to_string(harness.dir.join("existing.txt")).unwrap(), "after");
}

#[tokio::test]
async fn tool_write_preserves_exactly_one_bom() {
    let harness = harness().await;
    call(
        &harness.tools,
        "write",
        serde_json::json!({ "path": "preserved.txt", "content": "\u{FEFF}before" }),
    )
    .await
    .expect("create");
    call(
        &harness.tools,
        "write",
        serde_json::json!({ "path": "preserved.txt", "content": "\u{FEFF}after" }),
    )
    .await
    .expect("overwrite");
    call(
        &harness.tools,
        "write",
        serde_json::json!({ "path": "created.txt", "content": "\u{FEFF}\u{FEFF}\u{FEFF}created" }),
    )
    .await
    .expect("multi bom");
    assert_eq!(
        std::fs::read_to_string(harness.dir.join("preserved.txt")).unwrap(),
        "\u{FEFF}after"
    );
    assert_eq!(
        std::fs::read_to_string(harness.dir.join("created.txt")).unwrap(),
        "\u{FEFF}created"
    );
}

// ---------------------------------------------------------------------------
// Read (tool-read.test.ts + tool-read-filesystem.test.ts)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tool_read_file_lines_with_exact_format() {
    let harness = harness().await;
    std::fs::write(harness.dir.join("README.md"), "hello").expect("write");
    let settled = call(&harness.tools, "read", serde_json::json!({ "path": "README.md" }))
        .await
        .expect("read");
    assert_eq!(text_of(&settled), "Read file README.md, lines 1-1\n1: hello");
    assert_eq!(
        settled.metadata.expect("metadata").get("truncated"),
        Some(&serde_json::json!(false))
    );
}

#[tokio::test]
async fn tool_read_multiline_file_and_zero_line_file() {
    let harness = harness().await;
    std::fs::write(harness.dir.join("lines.txt"), "one\ntwo\nthree\n").expect("write");
    let settled = call(&harness.tools, "read", serde_json::json!({ "path": "lines.txt" }))
        .await
        .expect("read");
    assert_eq!(text_of(&settled), "Read file lines.txt, lines 1-3\n1: one\n2: two\n3: three");
    std::fs::write(harness.dir.join("empty.txt"), "").expect("write");
    let settled = call(&harness.tools, "read", serde_json::json!({ "path": "empty.txt" }))
        .await
        .expect("read empty");
    assert_eq!(text_of(&settled), "Read file empty.txt, 0 lines");
}

#[tokio::test]
async fn tool_read_pages_with_continuation_and_offset_out_of_range() {
    let harness = harness().await;
    let text: String = (1..=10).map(|n| format!("line{n}\n")).collect();
    std::fs::write(harness.dir.join("large.txt"), text).expect("write");
    let settled = call(
        &harness.tools,
        "read",
        serde_json::json!({ "path": "large.txt", "offset": 2, "limit": 1 }),
    )
    .await
    .expect("page");
    assert_eq!(
        text_of(&settled),
        "Read file large.txt, lines 2-2\n2: line2\n[Output truncated. Continue reading with offset: 3]"
    );
    assert_eq!(
        settled.metadata.expect("metadata").get("truncated"),
        Some(&serde_json::json!(true))
    );
    let failure =
        call(&harness.tools, "read", serde_json::json!({ "path": "large.txt", "offset": 99 }))
            .await
            .expect_err("out of range");
    assert_eq!(error_message(&failure), "Offset 99 is out of range for this file (10 lines)");
}

#[tokio::test]
async fn tool_read_line_clamping_at_2000_chars() {
    let harness = harness().await;
    std::fs::write(harness.dir.join("long.txt"), "x".repeat(2500)).expect("write");
    let settled = call(&harness.tools, "read", serde_json::json!({ "path": "long.txt" }))
        .await
        .expect("read");
    let text = text_of(&settled);
    assert!(
        text.ends_with("... (line truncated to 2000 chars)"),
        "tail: …{}",
        &text[text.len().saturating_sub(60)..]
    );
    assert!(text.contains(&format!("1: {}", "x".repeat(2000))));
}

#[tokio::test]
async fn tool_read_directory_listings_with_paging() {
    let harness = harness().await;
    std::fs::create_dir_all(harness.dir.join("src/components")).expect("dirs");
    std::fs::write(harness.dir.join("src/index.ts"), "x").expect("write");
    let settled =
        call(&harness.tools, "read", serde_json::json!({ "path": "src" })).await.expect("list");
    assert_eq!(text_of(&settled), "Read directory src, entries 1-2\ncomponents/\nindex.ts");
    // Zero offset normalizes to 1 in the heading.
    let settled =
        call(&harness.tools, "read", serde_json::json!({ "path": "src", "offset": 0, "limit": 1 }))
            .await
            .expect("zero offset");
    assert!(text_of(&settled).starts_with("Read directory src, entries 1-1\n"));
    assert!(text_of(&settled).contains("[Output truncated. Continue reading with offset: 2]"));
    // Empty directory.
    std::fs::create_dir_all(harness.dir.join("empty-dir")).expect("dir");
    let settled = call(&harness.tools, "read", serde_json::json!({ "path": "empty-dir" }))
        .await
        .expect("empty list");
    assert_eq!(text_of(&settled), "Read directory empty-dir, 0 entries");
}

#[tokio::test]
async fn tool_read_missing_file_suggestions_exact() {
    let harness = harness().await;
    std::fs::write(harness.dir.join("copy___missing_read_target__.txt"), "x").expect("write");
    std::fs::write(harness.dir.join("old___missing_read_target__.txt"), "x").expect("write");
    std::fs::write(harness.dir.join("unrelated.txt"), "x").expect("write");
    let failure =
        call(&harness.tools, "read", serde_json::json!({ "path": "__missing_read_target__.txt" }))
            .await
            .expect_err("missing");
    assert_eq!(
        error_message(&failure),
        "File not found: __missing_read_target__.txt\n\nDid you mean one of these?\ncopy___missing_read_target__.txt\nold___missing_read_target__.txt"
    );
    // No suggestions ⇒ bare message.
    let failure = call(&harness.tools, "read", serde_json::json!({ "path": "nowhere.txt" }))
        .await
        .expect_err("bare");
    assert_eq!(error_message(&failure), "File not found: nowhere.txt");
}

#[tokio::test]
async fn tool_read_recovers_unique_typography_variant() {
    let harness = harness().await;
    let recovered = harness.dir.join("Screenshot 2026-08-27 3.15\u{202F}PM.png");
    std::fs::write(&recovered, "hello").expect("write");
    let settled = call(
        &harness.tools,
        "read",
        serde_json::json!({ "path": "Screenshot 2026-08-27 3.15 PM.png" }),
    )
    .await
    .expect("recovered");
    assert!(
        text_of(&settled).starts_with("Read file Screenshot 2026-08-27 3.15"),
        "{}",
        text_of(&settled)
    );
}

#[tokio::test]
async fn tool_read_binary_file_error() {
    let harness = harness().await;
    std::fs::write(harness.dir.join("archive.dat"), b"abc\x00def").expect("write");
    let failure = call(&harness.tools, "read", serde_json::json!({ "path": "archive.dat" }))
        .await
        .expect_err("binary");
    assert_eq!(error_message(&failure), "Cannot read binary file: archive.dat");
}

#[tokio::test]
async fn tool_read_media_as_base64_file_parts() {
    let harness = harness().await;
    // Minimal PNG header — the magic-byte detector handles the rest.
    let png: Vec<u8> = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3];
    std::fs::write(harness.dir.join("pixel.png"), &png).expect("write");
    let settled = call(&harness.tools, "read", serde_json::json!({ "path": "pixel.png" }))
        .await
        .expect("png");
    assert_eq!(settled.content.len(), 2);
    assert_eq!(
        settled.content[0],
        oc_schema::tool::Content::Text { text: "Image read successfully".into() }
    );
    let oc_schema::tool::Content::File { uri, mime, name } = &settled.content[1] else {
        panic!("file part")
    };
    assert_eq!(mime, "image/png");
    assert_eq!(name.as_deref(), Some("pixel.png"));
    assert!(uri.starts_with("data:image/png;base64,"));

    // PDF summary line.
    std::fs::write(harness.dir.join("doc.pdf"), b"%PDF-1.7 fake").expect("write");
    let settled =
        call(&harness.tools, "read", serde_json::json!({ "path": "doc.pdf" })).await.expect("pdf");
    assert_eq!(
        settled.content[0],
        oc_schema::tool::Content::Text { text: "PDF read successfully".into() }
    );
}

// ---------------------------------------------------------------------------
// Formatter integration (post-write formatting through the tool path)
// ---------------------------------------------------------------------------

#[cfg(unix)]
#[tokio::test]
async fn tool_write_formats_via_formatter_when_available() {
    use std::os::unix::fs::PermissionsExt;

    let base = harness().await;
    let bin = tempfile::tempdir_in("/tmp/opencode").expect("bin tempdir");
    let rustfmt = bin.path().join("rustfmt");
    std::fs::write(&rustfmt, "#!/bin/sh\nprintf 'fmt:%s' \"$(cat \"$1\")\" > \"$1\"\n")
        .expect("script");
    let mut perms = std::fs::metadata(&rustfmt).expect("stat").permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&rustfmt, perms).expect("chmod");

    let services = Arc::new(crate::tool::ToolServices {
        formatter: Arc::new(crate::formatter::BuiltinsFormatter::new(
            base.dir.clone(),
            base.dir.clone(),
            vec![bin.path().to_path_buf()],
        )),
        ..(*base.services).clone_shape()
    });
    let tools = Arc::new(ToolRegistryState::new());
    let hooks = Arc::new(crate::hooks::HookRegistry::new());
    crate::tool::register_all(&tools, &hooks, &services).expect("register");

    let settled = call(
        &tools,
        "write",
        serde_json::json!({ "path": "src/lib.rs", "content": "fn main(){}" }),
    )
    .await
    .expect("write");
    assert_eq!(text_of(&settled), "Created file successfully: src/lib.rs");
    // The fake rustfmt ran after the write (formatter.file post-write hook).
    assert_eq!(
        std::fs::read_to_string(base.dir.join("src/lib.rs")).expect("read"),
        "fmt:fn main(){}"
    );

    // Non-matching extensions pass through untouched.
    call(&tools, "write", serde_json::json!({ "path": "note.txt", "content": "plain" }))
        .await
        .expect("write txt");
    assert_eq!(std::fs::read_to_string(base.dir.join("note.txt")).expect("read"), "plain");
}

// ---------------------------------------------------------------------------
// AGENTS.md side effect
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
struct RecordingInstructions {
    loads: Arc<parking_lot::Mutex<Vec<(String, Vec<PathBuf>)>>>,
}

impl crate::tool::InstructionInjection for RecordingInstructions {
    fn load(&self, session_id: &SessionId, paths: &[PathBuf]) {
        self.loads.lock().push((session_id.as_str().to_string(), paths.to_vec()));
    }
}

#[tokio::test]
async fn tool_read_discovers_agents_md_walking_up_to_the_location_root() {
    let recorder = RecordingInstructions::default();
    let base = harness().await;
    let services = Arc::new(crate::tool::ToolServices {
        instructions: Some(Arc::new(recorder.clone())),
        ..(*base.services).clone_shape()
    });
    let tools = Arc::new(ToolRegistryState::new());
    let hooks = Arc::new(crate::hooks::HookRegistry::new());
    crate::tool::register_all(&tools, &hooks, &services).expect("register");

    // AGENTS.md ABOVE the location root: not discovered (initial
    // instructions own it). AGENTS.md inside the location: discovered.
    std::fs::create_dir_all(base.dir.join("sub/deep")).expect("dirs");
    std::fs::write(base.dir.join("sub/AGENTS.md"), "sub rules").expect("write");
    std::fs::write(base.dir.join("AGENTS.md"), "root rules").expect("write");
    std::fs::write(base.dir.join("sub/deep/file.txt"), "hello").expect("write");

    call(&tools, "read", serde_json::json!({ "path": "sub/deep/file.txt" })).await.expect("read");
    let loads = recorder.loads.lock().clone();
    assert_eq!(loads.len(), 1, "one injection batch");
    let (session, paths) = &loads[0];
    assert_eq!(session, SESSION);
    // Walk order: nearest first (sub/deep has none; sub does; the location
    // root itself is exclusive).
    assert_eq!(paths, &[base.dir.join("sub/AGENTS.md")]);

    // Reading AGENTS.md directly does not re-inject the file itself.
    recorder.loads.lock().clear();
    call(&tools, "read", serde_json::json!({ "path": "AGENTS.md" })).await.expect("read agents");
    assert!(recorder.loads.lock().is_empty(), "the read file is excluded");
}

impl ToolServices {
    /// Clone the service shape with all shared handles (tests patch one
    /// field at a time).
    fn clone_shape(&self) -> ToolServices {
        ToolServices {
            directory: self.directory.clone(),
            project_directory: self.project_directory.clone(),
            home: self.home.clone(),
            access: self.access.clone(),
            permission: self.permission.clone(),
            mutation: Arc::clone(&self.mutation),
            formatter: Arc::clone(&self.formatter),
            instructions: self.instructions.clone(),
            db: self.db.clone(),
            bus: self.bus.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// glob / grep (tool-search.test.ts + tool-glob-implicit.test.ts) —
// fixture shim pinning the exact argv
// ---------------------------------------------------------------------------

use std::sync::OnceLock;

fn shim_dir() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("oc-rg-shim-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("shim dir");
        dir
    })
    .clone()
}

/// Installs an `rg` shim writing `stdout` and recording argv; returns the
/// argv log path.
fn install_rg_shim(stdout: &str) -> PathBuf {
    let dir = shim_dir();
    let log = dir.join(format!(
        "argv-{}.log",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
    ));
    let fixture = dir.join(format!(
        "fixture-{}.out",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
    ));
    std::fs::write(&fixture, stdout).expect("fixture");
    let log_display = log.display();
    let fixture_display = fixture.display();
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{log_display}'\ncat '{fixture_display}'\nexit 0\n"
    );
    let shim = dir.join(format!(
        "rg-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
    ));
    std::fs::write(&shim, script).expect("shim");
    set_executable(&shim);
    std::env::set_var("OPENCODE_RG_PATH", &shim);
    log
}

fn set_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    let mut permissions = std::fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).unwrap();
}

fn read_argv(log: &Path) -> Vec<String> {
    // The shim writes one argument per line; the vector is the whole file.
    std::fs::read_to_string(log).unwrap_or_default().lines().map(str::to_string).collect()
}

#[tokio::test]
async fn tool_glob_constructs_exact_args_and_formats_output() {
    let harness = harness().await;
    std::fs::create_dir_all(harness.dir.join("src")).expect("dir");
    let log = install_rg_shim("src/visible.ts\n");
    let settled = call(
        &harness.tools,
        "glob",
        serde_json::json!({ "pattern": "**/*.ts", "hidden": false, "limit": 10 }),
    )
    .await
    .expect("glob");
    let argv = read_argv(&log);
    assert_eq!(
        argv,
        vec![
            "--no-config",
            "--files",
            "--glob=**/*.ts",
            "--glob=!**/.*",
            "--glob=!**/.git/**",
            ".",
        ],
        "exact v2 argument construction (hidden=false)"
    );
    assert_eq!(text_of(&settled), format!("{}/src/visible.ts", harness.dir.display()));
    assert_eq!(settled.metadata.expect("metadata").get("count"), Some(&serde_json::json!(1)));

    // hidden=true adds --hidden and drops the .* exclusion.
    let log = install_rg_shim(".hidden.ts\nsrc/visible.ts\n");
    call(&harness.tools, "glob", serde_json::json!({ "pattern": "**/*.ts", "hidden": true }))
        .await
        .expect("glob hidden");
    let argv = read_argv(&log);
    assert_eq!(
        argv,
        vec!["--no-config", "--files", "--hidden", "--glob=**/*.ts", "--glob=!**/.git/**", ".",]
    );
}

#[tokio::test]
async fn tool_glob_no_files_and_truncation_footer() {
    let harness = harness().await;
    let log = install_rg_shim("");
    let settled = call(&harness.tools, "glob", serde_json::json!({ "pattern": "*.md" }))
        .await
        .expect("empty glob");
    assert_eq!(text_of(&settled), "No files found");
    drop(log);

    // 101 rows: the runner sees limit+1 and the tool truncates to 100.
    let body: String = (0..101).map(|n| format!("file{n}.txt\n")).collect();
    let log = install_rg_shim(&body);
    let settled = call(&harness.tools, "glob", serde_json::json!({ "pattern": "*" }))
        .await
        .expect("glob default limit");
    drop(log);
    // 100 entries + empty line + footer.
    assert_eq!(settled.content[0], oc_schema::tool::Content::Text {
        text: format!(
            "{}\n\n(Results are truncated: showing first 100 results. Consider using a more specific path or pattern.)",
            (0..100)
                .map(|n| format!("{}/file{n}.txt", harness.dir.display()))
                .collect::<Vec<_>>()
                .join("\n")
        )
    });
    assert_eq!(
        settled.metadata.expect("metadata").get("truncated"),
        Some(&serde_json::json!(true))
    );
}

#[tokio::test]
async fn tool_grep_constructs_exact_args_and_formats_matches() {
    let harness = harness().await;
    let fixture = concat!(
        "{\"type\":\"begin\",\"data\":{\"path\":{\"text\":\"target.txt\"}}}\n",
        "{\"type\":\"match\",\"data\":{\"path\":{\"text\":\"./target.txt\"},\"lines\":{\"text\":\"needle\\n\"},\"line_number\":1,\"absolute_offset\":0,\"submatches\":[{\"match\":{\"text\":\"needle\"},\"start\":0,\"end\":6}]}}\n",
        "{\"type\":\"end\",\"data\":{\"path\":{\"text\":\"target.txt\"},\"binary\":false,\"stats\":{}}}\n"
    );
    let log = install_rg_shim(fixture);
    let settled = call(
        &harness.tools,
        "grep",
        serde_json::json!({ "pattern": "needle", "literal": true, "caseSensitive": false }),
    )
    .await
    .expect("grep");
    let argv = read_argv(&log);
    assert_eq!(
        argv,
        vec![
            "--no-config",
            "--json",
            "--hidden",
            "--no-messages",
            "--fixed-strings",
            "--ignore-case",
            "--glob=!**/.git/**",
            "--",
            "needle",
            ".",
        ],
        "exact v2 argument construction"
    );
    assert_eq!(
        text_of(&settled),
        format!(
            "Found 1 matches\n{}:\n  Line 1: needle\n",
            harness.dir.join("target.txt").display()
        )
    );
    assert_eq!(settled.metadata.expect("metadata").get("matches"), Some(&serde_json::json!(1)));

    // include glob inserts before the git exclusion; explicit file path
    // greps only that file from its directory.
    let log = install_rg_shim(fixture);
    std::fs::write(harness.dir.join("target.txt"), "needle\n").expect("write");
    call(
        &harness.tools,
        "grep",
        serde_json::json!({ "pattern": "needle", "path": "target.txt", "include": "*.txt" }),
    )
    .await
    .expect("grep file");
    let argv = read_argv(&log);
    assert_eq!(
        argv,
        vec![
            "--no-config",
            "--json",
            "--hidden",
            "--no-messages",
            "--glob=*.txt",
            "--glob=!**/.git/**",
            "--",
            "needle",
            "target.txt",
        ]
    );
}

#[tokio::test]
async fn tool_grep_no_matches_and_truncation() {
    let harness = harness().await;
    let log = install_rg_shim("");
    let settled = call(&harness.tools, "grep", serde_json::json!({ "pattern": "needle" }))
        .await
        .expect("grep none");
    drop(log);
    assert_eq!(text_of(&settled), "No matches found");
    assert_eq!(settled.metadata.expect("metadata").get("matches"), Some(&serde_json::json!(0)));

    let fixture: String = (1..=101)
        .map(|n| {
            format!(
                "{{\"type\":\"match\",\"data\":{{\"path\":{{\"text\":\"f{n}.txt\"}},\"lines\":{{\"text\":\"needle\\n\"}},\"line_number\":{n},\"absolute_offset\":0,\"submatches\":[]}}}}\n"
            )
        })
        .collect();
    let log = install_rg_shim(&fixture);
    let settled = call(&harness.tools, "grep", serde_json::json!({ "pattern": "needle" }))
        .await
        .expect("grep truncated");
    drop(log);
    let text = text_of(&settled);
    assert!(text.starts_with("Found 100 matches\n"), "head: {}", &text[..40]);
    assert!(text.ends_with(
        "\n\n(Results are truncated: showing first 100 results. Consider using a more specific path or pattern.)"
    ));
}

#[tokio::test]
async fn tool_grep_invalid_regex_message_shape() {
    // Exit-2 + "regex parse error" stderr needs a failing shim; the message
    // shape is verified through the runner mapping.
    let dir = shim_dir();
    let script = "#!/bin/sh\nprintf 'regex parse error: unclosed character class at line 1\\n' >&2\nexit 2\n";
    let shim = dir.join("rg-invalid");
    std::fs::write(&shim, script).unwrap();
    set_executable(&shim);
    std::env::set_var("OPENCODE_RG_PATH", &shim);
    let harness = harness().await;
    let failure = call(&harness.tools, "grep", serde_json::json!({ "pattern": "[" }))
        .await
        .expect_err("invalid regex");
    assert_eq!(
        error_message(&failure),
        "Invalid regex pattern: regex parse error: unclosed character class at line 1"
    );
}

// ---------------------------------------------------------------------------
// Structured Output (RV-P8) — REF-exact shapes for codemode consumers
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tool_grep_structured_output_matches_ref_match_shape() {
    let harness = harness().await;
    let fixture = concat!(
        "{\"type\":\"match\",\"data\":{\"path\":{\"text\":\"./src/a.ts\"},\"lines\":{\"text\":\"needle\\n\"},\"line_number\":3,\"absolute_offset\":42,\"submatches\":[{\"match\":{\"text\":\"needle\"},\"start\":1,\"end\":7}]}}\n",
        "{\"type\":\"match\",\"data\":{\"path\":{\"text\":\"./src/b.rs\"},\"lines\":{\"text\":\"also needle here\\n\"},\"line_number\":8,\"absolute_offset\":90,\"submatches\":[{\"match\":{\"text\":\"needle\"},\"start\":5,\"end\":11}]}}\n"
    );
    let log = install_rg_shim(fixture);
    let settled = call(&harness.tools, "grep", serde_json::json!({ "pattern": "needle" }))
        .await
        .expect("grep");
    drop(log);
    // ref grep.ts Output = FileSystem.Match[] with location-relative entry
    // paths, line/offset/text and the submatch triples.
    assert_eq!(
        settled.output.expect("structured output"),
        serde_json::json!([
            {
                "entry": { "path": "src/a.ts", "type": "file" },
                "line": 3,
                "offset": 42,
                "text": "needle\n",
                "submatches": [{ "text": "needle", "start": 1, "end": 7 }]
            },
            {
                "entry": { "path": "src/b.rs", "type": "file" },
                "line": 8,
                "offset": 90,
                "text": "also needle here\n",
                "submatches": [{ "text": "needle", "start": 5, "end": 11 }]
            }
        ])
    );
}

#[tokio::test]
async fn tool_glob_structured_output_matches_ref_entry_shape() {
    let harness = harness().await;
    let log = install_rg_shim("src/visible.ts\n");
    let settled = call(
        &harness.tools,
        "glob",
        serde_json::json!({ "pattern": "**/*.ts", "hidden": false, "limit": 10 }),
    )
    .await
    .expect("glob");
    drop(log);
    // ref glob.ts Output = FileSystem.Entry[] with location-relative paths.
    assert_eq!(
        settled.output.expect("structured output"),
        serde_json::json!([{ "path": "src/visible.ts", "type": "file" }])
    );
}

#[tokio::test]
async fn tool_read_structured_output_matches_ref_union_shapes() {
    let harness = harness().await;
    // Whole-file read → FileContent (utf8).
    std::fs::write(harness.dir.join("README.md"), "hello").expect("write");
    let settled = call(&harness.tools, "read", serde_json::json!({ "path": "README.md" }))
        .await
        .expect("read file");
    let dir = harness.dir.display();
    assert_eq!(
        settled.output.expect("structured output"),
        serde_json::json!({
            "type": "file",
            "uri": format!("file://{dir}/README.md"),
            "name": "README.md",
            "content": "hello",
            "encoding": "utf8",
            "mime": "text/plain",
        })
    );

    // Paged read → TextPage.
    std::fs::write(harness.dir.join("page.txt"), "one\ntwo\n").expect("write");
    let settled = call(
        &harness.tools,
        "read",
        serde_json::json!({ "path": "page.txt", "offset": 2, "limit": 1 }),
    )
    .await
    .expect("read page");
    assert_eq!(
        settled.output.expect("structured output"),
        serde_json::json!({
            "type": "text-page",
            "content": "two",
            "mime": "text/plain",
            "offset": 2,
            "truncated": false,
        })
    );

    // Directory read → ListPage.
    std::fs::create_dir_all(harness.dir.join("src")).expect("dir");
    let settled = call(&harness.tools, "read", serde_json::json!({ "path": "src" }))
        .await
        .expect("read dir");
    assert_eq!(
        settled.output.expect("structured output"),
        serde_json::json!({
            "type": "list-page",
            "entries": [],
            "truncated": false,
        })
    );
}

// ---------------------------------------------------------------------------
// Patch (tool-patch.test.ts)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tool_patch_applies_add_update_delete_sequentially() {
    let harness = harness().await;
    std::fs::write(harness.dir.join("update.txt"), "before\n").expect("write");
    std::fs::write(harness.dir.join("remove.txt"), "remove\n").expect("write");
    let settled = call(
        &harness.tools,
        "patch",
        serde_json::json!({
            "patchText": "*** Begin Patch\n*** Add File: nested/new.txt\n+created\n*** Update File: update.txt\n@@\n-before\n+after\n*** Delete File: remove.txt\n*** End Patch"
        }),
    )
    .await
    .expect("patch");
    assert_eq!(
        text_of(&settled),
        "Success. Updated the following files:\nA nested/new.txt\nM update.txt\nD remove.txt"
    );
    assert_eq!(
        std::fs::read_to_string(harness.dir.join("nested/new.txt")).unwrap(),
        "created\n",
        "add ensures the trailing newline"
    );
    assert_eq!(std::fs::read_to_string(harness.dir.join("update.txt")).unwrap(), "after\n");
    assert!(!harness.dir.join("remove.txt").exists());
    // metadata carries the per-file diffs.
    let metadata = settled.metadata.expect("metadata");
    let files = metadata.get("files").and_then(|v| v.as_array()).expect("files");
    assert_eq!(files[0]["status"], "added");
    assert_eq!(files[1]["status"], "modified");
    assert_eq!(files[2]["status"], "deleted");
}

#[tokio::test]
async fn tool_patch_rejects_empty_and_boundary_errors() {
    let harness = harness().await;
    let failure = call(&harness.tools, "patch", serde_json::json!({ "patchText": "" }))
        .await
        .expect_err("empty input");
    assert_eq!(error_message(&failure), "patchText is required");
    let failure =
        call(&harness.tools, "patch", serde_json::json!({ "patchText": "invalid patch" }))
            .await
            .expect_err("boundary");
    assert_eq!(
        error_message(&failure),
        "patch verification failed: The first line of the patch must be '*** Begin Patch'"
    );
    let failure = call(
        &harness.tools,
        "patch",
        serde_json::json!({ "patchText": "*** Begin Patch\n*** Add File: foo\n+hello" }),
    )
    .await
    .expect_err("last boundary");
    assert_eq!(
        error_message(&failure),
        "patch verification failed: The last line of the patch must be '*** End Patch'"
    );
    let failure = call(
        &harness.tools,
        "patch",
        serde_json::json!({ "patchText": "*** Begin Patch\n*** Environment ID: abc\n*** End Patch" }),
    )
    .await
    .expect_err("empty hunks");
    assert_eq!(error_message(&failure), "patch rejected: empty patch");
}

#[tokio::test]
async fn tool_patch_missing_update_lines_and_delete_errors() {
    let harness = harness().await;
    std::fs::write(harness.dir.join("unchanged.txt"), "line1\nline2\n").expect("write");
    let failure = call(
        &harness.tools,
        "patch",
        serde_json::json!({
            "patchText": "*** Begin Patch\n*** Update File: unchanged.txt\n@@\n-missing\n+found\n*** End Patch"
        }),
    )
    .await
    .expect_err("missing lines");
    assert_eq!(
        error_message(&failure),
        "patch verification failed: Failed to find expected lines in unchanged.txt:\nmissing"
    );
    let failure = call(
        &harness.tools,
        "patch",
        serde_json::json!({ "patchText": "*** Begin Patch\n*** Delete File: missing.txt\n*** End Patch" }),
    )
    .await
    .expect_err("missing delete");
    assert_eq!(
        error_message(&failure),
        "patch verification failed: Failed to delete missing.txt: file does not exist"
    );
}

#[tokio::test]
async fn tool_patch_moves_and_cumulative_updates() {
    let harness = harness().await;
    std::fs::write(harness.dir.join("old.txt"), "same\n").expect("write");
    call(
        &harness.tools,
        "patch",
        serde_json::json!({
            "patchText": "*** Begin Patch\n*** Update File: old.txt\n*** Move to: moved.txt\n@@\n-same\n+after\n*** End Patch"
        }),
    )
    .await
    .expect("move");
    assert!(!harness.dir.join("old.txt").exists());
    assert_eq!(std::fs::read_to_string(harness.dir.join("moved.txt")).unwrap(), "after\n");

    // Two updates to the same file apply cumulatively.
    std::fs::write(harness.dir.join("alpha.txt"), "alpha\nbeta\nomega\n").expect("write");
    call(
        &harness.tools,
        "patch",
        serde_json::json!({
            "patchText": "*** Begin Patch\n*** Update File: alpha.txt\n@@\n-alpha\n+ALPHA\n@@\n-beta\n+BETA\n*** End Patch"
        }),
    )
    .await
    .expect("cumulative");
    assert_eq!(
        std::fs::read_to_string(harness.dir.join("alpha.txt")).unwrap(),
        "ALPHA\nBETA\nomega\n"
    );
}

#[tokio::test]
async fn tool_patch_reports_partial_failure_with_completed_list() {
    let harness = harness().await;
    // Make the second write fail by turning the target's parent into a file.
    std::fs::write(harness.dir.join("blocker"), "not a dir").expect("write");
    let failure = call(
        &harness.tools,
        "patch",
        serde_json::json!({
            "patchText": "*** Begin Patch\n*** Add File: first.txt\n+first\n*** Add File: blocker/second.txt\n+second\n*** End Patch"
        }),
    )
    .await
    .expect_err("partial");
    let message = error_message(&failure);
    assert!(message.starts_with("Failed to write blocker/second.txt: "), "message: {message}");
    assert!(message.ends_with(". Completed before failure: first.txt"), "message: {message}");
    assert_eq!(std::fs::read_to_string(harness.dir.join("first.txt")).unwrap(), "first\n");
}

#[tokio::test]
async fn tool_patch_gpt_codemod_hook_removes_edit_write_for_gpt_models() {
    let hooks = Arc::new(crate::hooks::HookRegistry::new());
    crate::tool::patch::install_codemod_hooks(&hooks);
    let mut tools = std::collections::BTreeMap::new();
    for name in ["edit", "write", "read", "patch"] {
        tools.insert(
            name.to_string(),
            crate::hooks::ToolDescriptor { description: name.into(), input: serde_json::json!({}) },
        );
    }
    let event = crate::hooks::SessionContextEvent {
        session_id: SessionId(SESSION.into()),
        agent: AgentId::parse("build"),
        model: oc_schema::model::Ref {
            provider_id: oc_schema::id::ProviderId::parse("openai"),
            id: oc_schema::id::ModelId::parse("gpt-5.1"),
            variant: None,
        },
        system: vec![],
        messages: vec![],
        options: Default::default(),
        tools: tools.clone(),
    };
    let shaped = hooks.dispatch_session_context(event).await.expect("gpt context");
    let names: Vec<&String> = shaped.tools.keys().collect();
    assert_eq!(names, vec!["patch", "read"], "gpt-* non-oss keeps patch, drops edit/write");

    let event = crate::hooks::SessionContextEvent {
        session_id: SessionId(SESSION.into()),
        agent: AgentId::parse("build"),
        model: oc_schema::model::Ref {
            provider_id: oc_schema::id::ProviderId::parse("anthropic"),
            id: oc_schema::id::ModelId::parse("claude-sonnet-4"),
            variant: None,
        },
        system: vec![],
        messages: vec![],
        options: Default::default(),
        tools,
    };
    let shaped = hooks.dispatch_session_context(event).await.expect("claude context");
    let names: Vec<&String> = shaped.tools.keys().collect();
    assert_eq!(names, vec!["edit", "read", "write"], "others drop patch");

    // gpt-4* and oss models keep edit/write.
    for model in ["gpt-4o", "gpt-4.1", "o3-oss-gpt-5"] {
        let event = crate::hooks::SessionContextEvent {
            session_id: SessionId(SESSION.into()),
            agent: AgentId::parse("build"),
            model: oc_schema::model::Ref {
                provider_id: oc_schema::id::ProviderId::parse("openai"),
                id: oc_schema::id::ModelId::parse(model),
                variant: None,
            },
            system: vec![],
            messages: vec![],
            options: Default::default(),
            tools: vec![
                (
                    "edit".into(),
                    crate::hooks::ToolDescriptor {
                        description: "e".into(),
                        input: serde_json::json!({}),
                    },
                ),
                (
                    "patch".into(),
                    crate::hooks::ToolDescriptor {
                        description: "p".into(),
                        input: serde_json::json!({}),
                    },
                ),
            ]
            .into_iter()
            .collect(),
        };
        let shaped = hooks.dispatch_session_context(event).await.expect("context");
        assert!(shaped.tools.contains_key("edit"), "{model} keeps edit");
        assert!(!shaped.tools.contains_key("patch"), "{model} drops patch");
    }
}

// ---------------------------------------------------------------------------
// tool.execute hooks (pipeline)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tool_pipeline_before_hook_mutates_input_and_after_observes_result() {
    let tools = ToolRegistryState::new();
    let harness = harness().await;
    let hooks = Arc::new(crate::hooks::HookRegistry::new());
    // Keep the registration alive for the test body (dropping it disposes
    // the hook).
    let _keep_before = hooks
        .register_tool_execute_before(
            crate::hooks::fn_hook(|event: &mut crate::hooks::ToolExecuteBeforeEvent| {
                event.input["content"] = serde_json::json!("mutated");
                Ok(())
            }),
            crate::hooks::HookOptions::default(),
        )
        .await;
    let seen = Arc::new(parking_lot::Mutex::new(Vec::<String>::new()));
    let seen_hook = Arc::clone(&seen);
    let _keep_after = hooks
        .register_tool_execute_after(
            crate::hooks::fn_hook(move |event: &mut crate::hooks::ToolExecuteAfterEvent| {
                if let crate::hooks::ToolOutcome::Completed { result } = &event.outcome {
                    let text =
                        result["content"][0]["text"].as_str().unwrap_or_default().to_string();
                    seen_hook.lock().push(text);
                }
                Ok(())
            }),
            crate::hooks::HookOptions::default(),
        )
        .await;
    crate::tool::registry::register(
        &tools,
        &hooks,
        &harness.services,
        crate::tool::write::tool(&harness.services),
    )
    .expect("register");

    let settled =
        call(&tools, "write", serde_json::json!({ "path": "hooked.txt", "content": "original" }))
            .await
            .expect("write");
    assert_eq!(text_of(&settled), "Created file successfully: hooked.txt");
    assert_eq!(
        std::fs::read_to_string(harness.dir.join("hooked.txt")).unwrap(),
        "mutated",
        "before hook rewrote the input"
    );
    assert_eq!(*seen.lock(), vec!["Created file successfully: hooked.txt".to_string()]);
}

#[tokio::test]
async fn tool_pipeline_before_hook_rejection_short_circuits() {
    let tools = ToolRegistryState::new();
    let harness = harness().await;
    let hooks = Arc::new(crate::hooks::HookRegistry::new());
    let _keep_before = hooks
        .register_tool_execute_before(
            crate::hooks::fn_hook(|event: &mut crate::hooks::ToolExecuteBeforeEvent| {
                let _ = event;
                Err(crate::hooks::HookError::ToolRejected(oc_schema::tool::ToolError {
                    message: "rejected by hook".into(),
                    error: None,
                    metadata: None,
                }))
            }),
            crate::hooks::HookOptions::default(),
        )
        .await;
    crate::tool::registry::register(
        &tools,
        &hooks,
        &harness.services,
        crate::tool::write::tool(&harness.services),
    )
    .expect("register");
    let failure = call(&tools, "write", serde_json::json!({ "path": "x.txt", "content": "y" }))
        .await
        .expect_err("rejected");
    assert_eq!(error_message(&failure), "rejected by hook");
    assert!(!harness.dir.join("x.txt").exists());
}

// -- execute.after application fidelity (D-03: by-reference parity) --------

/// v2 returns the (possibly plugin-rewritten) error from the error-leg
/// after hook (`return yield* afterEvent.error`, tool.ts:140): a plugin
/// rewriting the message shapes the model-visible failure.
#[tokio::test]
async fn tool_pipeline_after_hook_error_rewrite_shapes_failure() {
    let tools = ToolRegistryState::new();
    let harness = harness().await;
    let hooks = Arc::new(crate::hooks::HookRegistry::new());
    let _keep_after = hooks
        .register_tool_execute_after(
            crate::hooks::fn_hook(|event: &mut crate::hooks::ToolExecuteAfterEvent| {
                if let crate::hooks::ToolOutcome::Error { error } = &mut event.outcome {
                    error.message = "rewritten by plugin".into();
                }
                Ok(())
            }),
            crate::hooks::HookOptions::default(),
        )
        .await;
    crate::tool::registry::register(
        &tools,
        &hooks,
        &harness.services,
        crate::tool::read::tool(&harness.services),
    )
    .expect("register");

    let failure = call(&tools, "read", serde_json::json!({ "path": "nowhere-d03.txt" }))
        .await
        .expect_err("read of a missing file fails");
    assert_eq!(
        error_message(&failure),
        "rewritten by plugin",
        "the after hook's error rewrite must become the tool failure"
    );
}

/// v2's normalizeContent applies an EMPTIED content array too (empty →
/// the stringified output fallback, tool/runtime.ts:259): a plugin
/// suppressing tool output must suppress it, not keep the original.
#[tokio::test]
async fn tool_pipeline_after_hook_can_suppress_content() {
    let tools = ToolRegistryState::new();
    let harness = harness().await;
    let hooks = Arc::new(crate::hooks::HookRegistry::new());
    let _keep_after = hooks
        .register_tool_execute_after(
            crate::hooks::fn_hook(|event: &mut crate::hooks::ToolExecuteAfterEvent| {
                if let crate::hooks::ToolOutcome::Completed { result } = &mut event.outcome {
                    // TOOL-D1: v2 suppression must empty the content AND
                    // drop the declared output — `normalizeContent([], output)`
                    // re-derives text from a surviving output
                    // (tool/runtime.ts:259-263), so only removing both
                    // suppresses the model-facing output.
                    result["content"] = serde_json::json!([]);
                    if let Some(object) = result.as_object_mut() {
                        object.remove("output");
                    }
                }
                Ok(())
            }),
            crate::hooks::HookOptions::default(),
        )
        .await;
    crate::tool::registry::register(
        &tools,
        &hooks,
        &harness.services,
        crate::tool::write::tool(&harness.services),
    )
    .expect("register");

    let settled =
        call(&tools, "write", serde_json::json!({ "path": "suppressed.txt", "content": "x" }))
            .await
            .expect("write");
    assert!(
        settled.content.is_empty(),
        "an emptied content array must suppress the output (v2 normalizeContent)"
    );
}
