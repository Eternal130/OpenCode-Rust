//! Regression for CI-RUNTIME-GATES-RED legs 2+3 (CI run 39468671027):
//! `actions/upload-artifact@v4` does not preserve the executable bit,
//! and the runtime-gates jobs consume the whole `target/` dir from the
//! artifact — so the shipped `target/debug/oc-server` arrived as a
//! regular 0644 file. `build_binary()` only checked `is_file()`, handed
//! that stale file to `Command::new`, and every spawn died with
//! `spawn .../target/debug/oc-server: Permission denied (os error 13)`
//! (p10 battery leg + the §12.3 real_db_null_census leg; the gates
//! summary leg was red purely downstream of these).
//!
//! These tests pin the [`parity::rust_server::ensure_binary`] contract
//! hermetically — no cargo invocation, no server rebuild: a
//! present-but-unexecutable file is never returned to a caller.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;

use parity::rust_server::{ensure_binary, is_executable_file};

/// A fresh per-test scratch path (contents are a few bytes; /tmp on the
/// gate box is ephemeral).
fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join(format!("parity-server-binary-exec-{}-{tag}", std::process::id()));
    fs::create_dir_all(&dir).expect("create scratch dir");
    dir.join("binary-under-test")
}

fn write_with_mode(path: &Path, mode: u32) {
    fs::write(path, b"stale artifact\n").expect("write file");
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("set mode");
}

/// Raw wait status 0 == exited with code 0.
fn ok_status() -> ExitStatus {
    ExitStatus::from_raw(0)
}

/// Raw wait status 1<<8 == exited with code 1.
fn fail_status() -> ExitStatus {
    ExitStatus::from_raw(1 << 8)
}

#[test]
fn server_binary_exec_bit_classification() {
    let path = scratch("classify");
    assert!(!is_executable_file(&path), "a missing file is never executable");
    fs::create_dir_all(&path).expect("create dir");
    assert!(!is_executable_file(&path), "a directory is never executable");
    fs::remove_dir(&path).expect("remove dir");
    write_with_mode(&path, 0o644);
    assert!(
        !is_executable_file(&path),
        "0644 — the upload-artifact v4 shape — is NOT executable"
    );
    write_with_mode(&path, 0o755);
    assert!(is_executable_file(&path), "0755 is executable");
    write_with_mode(&path, 0o700);
    assert!(is_executable_file(&path), "0700 is executable");
}

#[test]
fn server_binary_exec_bit_reused_when_already_executable() {
    let path = scratch("reuse");
    write_with_mode(&path, 0o755);
    let returned = ensure_binary(&path, || {
        panic!("an already-executable binary must not trigger a build")
    })
    .expect("reuse the existing executable");
    assert_eq!(returned, path);
}

/// THE regression (CI run 39468671027): the artifact-shipped binary
/// exists but carries no exec bit. It must be REMOVED before building —
/// cargo does not relink while its fingerprint is fresh and the output
/// file exists — and the returned path must be the fresh executable.
#[test]
fn server_binary_exec_bit_rebuilt_when_artifact_lost_the_bit() {
    let path = scratch("rebuild");
    write_with_mode(&path, 0o644);
    let mut build_ran = false;
    let returned = ensure_binary(&path, || {
        build_ran = true;
        assert!(
            !path.exists(),
            "the stale non-executable file must be removed BEFORE the build runs"
        );
        write_with_mode(&path, 0o755);
        Ok(ok_status())
    })
    .expect("rebuild over the stale artifact");
    assert!(build_ran, "a non-executable artifact must not short-circuit the build");
    assert_eq!(returned, path);
    assert!(is_executable_file(&returned));
}

#[test]
fn server_binary_exec_bit_never_returns_an_unexecutable_path() {
    let path = scratch("guard");
    write_with_mode(&path, 0o644);
    let error = ensure_binary(&path, || {
        // Simulates cargo leaving a non-executable output: the guard is
        // the contract — error out instead of handing back a broken path
        // that would EACCES at spawn time.
        write_with_mode(&path, 0o644);
        Ok(ok_status())
    })
    .expect_err("an unexecutable build result must be an error, not a path");
    assert!(
        error.to_string().contains("is not an executable file"),
        "unexpected error: {error}"
    );
}

#[test]
fn server_binary_exec_bit_build_failure_is_an_error() {
    let path = scratch("buildfail");
    let error =
        ensure_binary(&path, || Ok(fail_status())).expect_err("a failing build must be an error");
    assert!(error.to_string().contains("status"), "unexpected error: {error}");
}

#[test]
fn server_binary_exec_bit_unremovable_stale_path_is_an_error() {
    let path = scratch("unremovable");
    fs::create_dir_all(&path).expect("create a directory at the binary path");
    let error = ensure_binary(&path, || {
        panic!("the build must not run when the stale path cannot be removed")
    })
    .expect_err("an unremovable stale path must be an error");
    assert!(error.to_string().contains("cannot remove"), "unexpected error: {error}");
    fs::remove_dir(&path).expect("cleanup");
}
