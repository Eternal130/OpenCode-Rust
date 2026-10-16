//! Build script for the vendored miniaudio sys crate.
//!
//! Compiles the single translation unit (`src/shim.c` → miniaudio 0.11.22
//! implementation) with `cc`. Bindings are PRE-GENERATED and committed at
//! `src/bindings.rs` (see README.md for the regeneration recipe), so a clean
//! build needs neither bindgen nor libclang nor any system audio dev
//! headers. On Linux miniaudio's ALSA/Pulse backends resolve their entry
//! points at runtime via `dlopen`, so we only link the eternal trio
//! (`dl`, `pthread`, `m`) — never `-lasound`/`-lpulse`.

use std::env;

fn main() {
    let mut build = cc::Build::new();
    build
        .cpp(false)
        .std("c99")
        .define("MINIAUDIO_IMPLEMENTATION", "")
        .include("src")
        .file("src/shim.c")
        .flag_if_supported("-Wno-unused-function")
        .warnings(false);
    build.compile("miniaudio");

    // Silence the miniaudio implementation TU (vendored C, warnings off in
    // CI too). Keep rebuild triggers tight for incremental correctness.
    println!("cargo:rerun-if-changed=src/shim.c");
    println!("cargo:rerun-if-changed=src/miniaudio/miniaudio.h");
    println!("cargo:rerun-if-env-changed=CC");

    let target = env::var("TARGET").unwrap_or_default();
    if target.contains("linux") {
        // Runtime dlopen backend loading (see shim.c): link the loader, not
        // the audio libraries.
        println!("cargo:rustc-link-lib=dylib=dl");
        println!("cargo:rustc-link-lib=dylib=pthread");
        println!("cargo:rustc-link-lib=dylib=m");
    }
}
