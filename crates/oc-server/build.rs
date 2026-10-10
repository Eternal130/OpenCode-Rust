//! Build stamp for the daemon build-identity marker (B4-H3): the
//! managed daemon reports WHO BUILT IT at probe time via the
//! `x-opencode-build` header, so a client binary can tell two builds
//! of the SAME pinned version string ("2.0.24" across betas) apart
//! and replace a stale daemon after `opencode-rust upgrade`.
//!
//! The stamp reuses oc-cli's build-stamp recipe (crates/oc-cli/
//! build.rs, MIGRATION §2.4): `OC_RELEASE_TAG` (the release workflow
//! sets it to the git tag; everything else gets "dev") + the short git
//! hash — pre-concatenated here into ONE env var so `BUILD_STAMP` can
//! be a `const &str` via `option_env!`.

fn main() {
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=OC_RELEASE_TAG");
    println!("cargo:rustc-env=OC_BUILD_STAMP={}", stamp());
}

/// `<release-tag>@<git-hash>` — differs across betas (tag) and across
/// commits from source (hash), exactly the granularity the upgrade
/// flow needs.
fn stamp() -> String {
    let tag = std::env::var("OC_RELEASE_TAG").unwrap_or_else(|_| "dev".to_owned());
    format!("{tag}@{}", git_hash())
}

/// Short git hash of the working tree (`unknown` outside a clone) —
/// the same helper as oc-cli's build.rs.
fn git_hash() -> String {
    let output = std::process::Command::new("git")
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
