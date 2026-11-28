//! Version stamping (MIGRATION §2.4): the release binary must report
//! exactly `2.0.24` (service version negotiation is exact-string;
//! `opencode-rust --version` prints `opencode-rust v2.0.24`). The hidden
//! `--version-rust` flag exposes the port's own build stamp:
//! git hash + rustc + profile, captured here at compile time.

use std::process::Command;

fn main() {
    // Re-stamp when HEAD moves. The literal `../../.git/HEAD` does not exist
    // in a linked worktree (`.git` is a file there), and cargo re-runs a build
    // script whose rerun-if-changed path is missing — which recompiled this
    // crate on EVERY cargo invocation from a worktree (~1m43s each). Ask git
    // where HEAD actually lives; outside a checkout nothing is watched.
    if let Some(head) = git_path("HEAD") {
        println!("cargo:rerun-if-changed={}", head.display());
    }
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=OC_RELEASE_TAG");
    println!("cargo:rustc-env=OC_BUILD_GIT_HASH={}", git_hash());
    println!("cargo:rustc-env=OC_BUILD_PROFILE={}", profile());
    // Lane C: the release tag this binary was built from (the release
    // workflow sets it to the git tag; everything else gets "dev" and
    // the update channel stays off for that build).
    let tag = std::env::var("OC_RELEASE_TAG").unwrap_or_else(|_| "dev".to_owned());
    println!("cargo:rustc-env=OC_RELEASE_TAG={tag}");
}

/// Short git hash of the working tree (`unknown` outside a clone).
fn git_hash() -> String {
    let output = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output();
    match output {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        }
        _ => "unknown".to_owned(),
    }
}

/// The absolute path of a git path spec (`HEAD` here), resolved through git so
/// that it is right in the main checkout (`.git/HEAD`) and in a linked worktree
/// (`<git-common-dir>/worktrees/<name>/HEAD`). `None` when git cannot resolve
/// it or the file does not exist — then nothing is watched.
fn git_path(spec: &str) -> Option<std::path::PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--git-path", spec])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if raw.is_empty() {
        return None;
    }
    let path = std::path::PathBuf::from(raw);
    let absolute = if path.is_absolute() {
        path
    } else {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
    };
    std::fs::canonicalize(absolute).ok()
}

/// The compilation profile (the stamp means most for `release`).
fn profile() -> String {
    std::env::var("PROFILE").unwrap_or_else(|_| "unknown".to_owned())
}
