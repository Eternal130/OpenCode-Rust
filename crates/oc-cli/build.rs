//! Version stamping (MIGRATION §2.4): the release binary must report
//! exactly `2.0.24` (service version negotiation is exact-string;
//! `opencode-rust --version` prints `opencode-rust v2.0.24`). The hidden
//! `--version-rust` flag exposes the port's own build stamp:
//! git hash + rustc + profile, captured here at compile time.

use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=../../.git/HEAD");
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

/// The compilation profile (the stamp means most for `release`).
fn profile() -> String {
    std::env::var("PROFILE").unwrap_or_else(|_| "unknown".to_owned())
}
