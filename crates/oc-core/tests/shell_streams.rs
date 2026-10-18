//! TOOLS-EXEC regressions (ledger TOOL-C1, TOOL-C2): the shell runtime
//! must pump stdout/stderr CONCURRENTLY into the combined capture (no
//! >64 KiB pipe interlock, arrival-order interleave preserved, timeout
//! kills TERM → 3 s → KILL), and every production spawn path must
//! dispatch the `shell.create.before` hook before the permission
//! pipeline runs.
//!
//! Hermetic: capture dirs sandboxed under /tmp/opencode, /bin/sh only,
//! no network; child processes are owned by the Shell service (which
//! signals only its own process groups).

use std::sync::Arc;

use oc_core::hooks::{HookError, HookHandler, HookOptions, HookRegistry, ShellCreateBeforeEvent};
use oc_core::shell::Shell;
use oc_schema::shell::{CreateInput, Status};
use oc_util::determinism::{AscendingIdGen, IdGen, SequentialRng, SystemClock};

fn ids() -> Arc<dyn IdGen> {
    Arc::new(AscendingIdGen::new(Arc::new(SystemClock), Arc::new(SequentialRng::new())))
}

fn service(tmp: &tempfile::TempDir) -> Shell {
    Shell::new(tmp.path(), "toolsc1", ids(), Arc::new(SystemClock))
}

fn sandbox() -> tempfile::TempDir {
    tempfile::tempdir_in("/tmp/opencode").expect("sandbox tempdir under /tmp/opencode")
}

fn create_input(command: impl Into<String>, cwd: &tempfile::TempDir) -> CreateInput {
    CreateInput {
        command: command.into(),
        cwd: Some(cwd.path().to_string_lossy().to_string()),
        timeout: Some(10_000),
        metadata: None,
    }
}

// ---------------------------------------------------------------------------
// TOOL-C1: concurrent streams
// ---------------------------------------------------------------------------

/// Alternating stderr→stdout writes with wide timing margins: `err{i}`
/// lands a full 60 ms before `out{i+1}` is even written, so a CONCURRENT
/// pump must place `err1` before `out2` in the combined capture. The
/// sequential pump (stdout drained to EOF first — the bug) writes every
/// `out*` line before every `err*` line.
#[tokio::test]
async fn interleaved_streams() {
    let tmp = sandbox();
    let shell = service(&tmp);
    let mut command = String::new();
    for i in 1..=5 {
        command.push_str(&format!(
            "printf 'err{i}\\n' >&2; sleep 0.06; printf 'out{i}\\n'; sleep 0.06; "
        ));
    }
    let info = shell
        .create(create_input(command, &tmp), Some("/bin/sh"), None, None)
        .await
        .expect("create");
    let result = shell.result(info).await;
    assert_eq!(result.info.status, Status::Exited, "command must run to completion");
    let capture = result.capture.expect("capture");
    let err1 = capture.output.find("err1").unwrap_or_else(|| {
        panic!("err1 missing from capture: {}", capture.output)
    });
    let out2 = capture.output.find("out2").unwrap_or_else(|| {
        panic!("out2 missing from capture: {}", capture.output)
    });
    assert!(
        err1 < out2,
        "stderr chunks must interleave by arrival order; capture was: {}",
        capture.output
    );
}

/// 128 KiB of stderr (> the 64 KiB pipe buffer) write-blocks the child
/// until the service drains stderr. A sequential pump parked on stdout
/// waits for an EOF that never arrives while the child is blocked, so
/// the timeout kills a healthy command (the false-timeout interlock).
/// Concurrent pumps drain both pipes and the command completes.
#[tokio::test]
async fn stderr_beyond_pipe_capacity_completes_without_timeout() {
    let tmp = sandbox();
    let shell = service(&tmp);
    let command =
        "head -c 131072 /dev/zero | tr '\\0' 'B' >&2; printf done".to_string();
    let info = shell
        .create(create_input(command, &tmp), Some("/bin/sh"), None, None)
        .await
        .expect("create");
    let result = shell.result(info).await;
    assert_eq!(
        result.info.status,
        Status::Exited,
        "a healthy command with >64KiB stderr must not hit the timeout interlock"
    );
    let capture = result.capture.expect("capture");
    assert!(
        capture.output.contains("done"),
        "stdout tail must be captured after the large stderr; capture: {} bytes",
        capture.output.len()
    );
    assert!(
        capture.output.bytes().filter(|byte| *byte == b'B').count() >= 131_000,
        "the full stderr stream must be drained into the capture"
    );
}

/// v2 kill semantics (`forceKillAfter: 3s`): the timeout delivers TERM
/// first and only escalates to KILL if the group survives. A child that
/// traps TERM and flushes a marker before exiting proves the FIRST
/// signal was TERM — an immediate SIGKILL loses the marker.
#[tokio::test]
async fn timeout_term_then_kill_escalation() {
    let tmp = sandbox();
    let shell = service(&tmp);
    let command = "trap 'printf TERM-RECEIVED >&2; exit 0' TERM; printf ready; \
                   while true; do sleep 0.05; done"
        .to_string();
    let info = shell
        .create(create_input(command, &tmp), Some("/bin/sh"), None, None)
        .await
        .expect("create");
    let result = shell.result(info).await;
    assert_eq!(result.info.status, Status::Timeout, "the timeout must settle the status");
    let capture = result.capture.expect("capture");
    assert!(capture.output.contains("ready"), "pre-timeout output is captured");
    assert!(
        capture.output.contains("TERM-RECEIVED"),
        "timeout kill must start with SIGTERM (TERM trap marker missing); capture: {}",
        capture.output
    );
}

// ---------------------------------------------------------------------------
// TOOL-C2: shell.create.before production dispatch
// ---------------------------------------------------------------------------

struct RewriteCommand;

#[async_trait::async_trait]
impl HookHandler<ShellCreateBeforeEvent> for RewriteCommand {
    async fn call(&self, event: &mut ShellCreateBeforeEvent) -> Result<(), HookError> {
        event.command = "printf hooked".into();
        event.env.insert("TOOL_C2".to_string(), Some("1".into()));
        Ok(())
    }
}

struct RejectShell;

#[async_trait::async_trait]
impl HookHandler<ShellCreateBeforeEvent> for RejectShell {
    async fn call(&self, _event: &mut ShellCreateBeforeEvent) -> Result<(), HookError> {
        Err(HookError::Handler("shell rejected by policy".into()))
    }
}

/// The service-level spawn path dispatches `shell.create.before` and the
/// hook's mutations (command + env) reach the spawned process.
#[tokio::test]
async fn create_before_hook() {
    let tmp = sandbox();
    let hooks = Arc::new(HookRegistry::new());
    hooks
        .register_shell_create_before(Arc::new(RewriteCommand), HookOptions::default())
        .await;
    let shell = service(&tmp).with_hooks(hooks);
    let info = shell
        .create(create_input("printf original", &tmp), Some("/bin/sh"), None, None)
        .await
        .expect("create");
    assert_eq!(info.command, "printf hooked", "the hook rewrites the invocation");
    let result = shell.result(info).await;
    assert_eq!(result.info.status, Status::Exited);
    let capture = result.capture.expect("capture");
    assert!(
        capture.output.contains("hooked"),
        "the spawned process runs the rewritten command; capture: {}",
        capture.output
    );
}

/// A hook rejection fails the create — nothing spawns.
#[tokio::test]
async fn create_before_hook_rejection_fails_create() {
    let tmp = sandbox();
    let hooks = Arc::new(HookRegistry::new());
    hooks
        .register_shell_create_before(Arc::new(RejectShell), HookOptions::default())
        .await;
    let shell = service(&tmp).with_hooks(hooks);
    let outcome = shell
        .create(create_input("printf never", &tmp), Some("/bin/sh"), None, None)
        .await;
    let error = outcome.expect_err("a rejected hook must fail the create");
    assert!(
        error.to_string().contains("rejected"),
        "the hook error surfaces to the caller: {error}"
    );
}
