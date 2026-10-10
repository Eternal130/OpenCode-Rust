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
//!
//! Second responsibility (R-04): staging the web-UI app archive into
//! `OUT_DIR` under a fixed name so `include_bytes!` has a literal path
//! (see [`app_archive::embedded_archive`]).

use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=OC_RELEASE_TAG");
    println!("cargo:rustc-env=OC_BUILD_STAMP={}", stamp());
    embed_app_archive();
}

/// The fixed staging name inside `OUT_DIR` (always written — empty file
/// when no archive — so a build without `OPENCODE_APP_ARCHIVE` never
/// picks up a stale copy from a previous env-set build).
const STAGED_ARCHIVE: &str = "embedded-app-archive.bin";

/// R-04 web-UI embed: when `OPENCODE_APP_ARCHIVE` (an
/// `app-archive.json` produced by the release workflow's `web-assets`
/// job, see `tools/web-assets/package.ts`) is set at BUILD time, copies
/// it into `OUT_DIR` for compile-time embedding. Absent/unset → an
/// empty staged file → `embedded_archive()` is `None` → API-only
/// binary (every dev build).
fn embed_app_archive() {
    let out_dir = std::env::var("OUT_DIR").expect("cargo sets OUT_DIR");
    let dest = Path::new(&out_dir).join(STAGED_ARCHIVE);
    println!("cargo:rerun-if-env-changed=OPENCODE_APP_ARCHIVE");
    match std::env::var("OPENCODE_APP_ARCHIVE") {
        Ok(source) if !source.is_empty() => {
            println!("cargo:rerun-if-changed={source}");
            let bytes = std::fs::read(&source).unwrap_or_else(|error| {
                panic!("OPENCODE_APP_ARCHIVE={source:?}: unreadable: {error}");
            });
            std::fs::write(&dest, &bytes).unwrap_or_else(|error| {
                panic!("failed to stage the app archive into OUT_DIR: {error}");
            });
            println!(
                "cargo:warning=oc-server: embedding web-UI app archive {} ({} bytes)",
                source,
                bytes.len()
            );
        }
        _ => {
            std::fs::write(&dest, b"").expect("write empty staged archive");
        }
    }
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
