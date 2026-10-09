//! CLI: `cargo run -p openapi-gen [-- --check | --stdout routes|types|methods] [--spec PATH]`

#![allow(clippy::too_many_lines)]

use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut check = false;
    let mut stdout: Option<String> = None;
    let mut spec: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--check" => check = true,
            "--stdout" => {
                i += 1;
                stdout = Some(args.get(i).cloned().unwrap_or_else(|| {
                    eprintln!("--stdout requires routes|types|methods");
                    std::process::exit(2);
                }));
            }
            "--spec" => {
                i += 1;
                spec = Some(args.get(i).cloned().unwrap_or_else(|| {
                    eprintln!("--spec requires a path");
                    std::process::exit(2);
                }));
            }
            other => {
                eprintln!(
                    "unknown argument {other:?}; expected --check, --stdout <which>, --spec <path>"
                );
                return ExitCode::from(2);
            }
        }
        i += 1;
    }

    let spec_json = match spec.as_deref() {
        Some(path) => std::fs::read_to_string(path).unwrap_or_else(|e| {
            eprintln!("failed to read spec {path}: {e}");
            std::process::exit(2);
        }),
        None => std::fs::read_to_string(openapi_gen::spec_path()).unwrap_or_else(|e| {
            eprintln!("failed to read pinned spec {}: {e}", openapi_gen::spec_path());
            std::process::exit(2);
        }),
    };
    let generated = openapi_gen::generate_from_spec(&spec_json);
    for warning in &generated.warnings {
        eprintln!("warning: {warning}");
    }
    eprintln!(
        "generated {} ops (spec sha256 {}), {} type defs",
        generated.operation_count,
        generated.spec_sha256,
        generated.types_rs.matches("pub struct").count()
            + generated.types_rs.matches("pub enum").count()
    );

    if let Some(which) = stdout.as_deref() {
        let text = match which {
            "routes" => &generated.routes_rs,
            "types" => &generated.types_rs,
            "methods" => &generated.methods_rs,
            other => {
                eprintln!("unknown --stdout target {other:?}");
                return ExitCode::from(2);
            }
        };
        print!("{text}");
        return ExitCode::SUCCESS;
    }

    let root = workspace_root();
    let files = [
        (generated.routes_rs.as_str(), "crates/oc-server/src/routes.gen.rs"),
        (generated.types_rs.as_str(), "crates/oc-client/src/types.gen.rs"),
        (generated.methods_rs.as_str(), "crates/oc-client/src/methods.gen.rs"),
    ];

    if check {
        let mut drift = false;
        for (text, rel) in files {
            let path = root.join(rel);
            match std::fs::read_to_string(&path) {
                Ok(current) => {
                    if openapi_gen::normalize_ws(&current) != openapi_gen::normalize_ws(text) {
                        drift = true;
                        eprintln!(
                            "DRIFT: {rel} differs from regeneration ({} vs {} bytes)",
                            current.len(),
                            text.len()
                        );
                    }
                }
                Err(e) => {
                    drift = true;
                    eprintln!("DRIFT: cannot read {rel}: {e}");
                }
            }
        }
        if drift {
            eprintln!("run `cargo run -p openapi-gen && cargo fmt` to regenerate");
            return ExitCode::FAILURE;
        }
        eprintln!("ok: generated artifacts match regeneration");
        return ExitCode::SUCCESS;
    }

    for (text, rel) in files {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap_or_else(|e| {
                eprintln!("failed to create {}: {e}", parent.display());
                std::process::exit(2);
            });
        }
        std::fs::write(&path, text).unwrap_or_else(|e| {
            eprintln!("failed to write {}: {e}", path.display());
            std::process::exit(2);
        });
        println!("wrote {}", path.display());
    }
    ExitCode::SUCCESS
}

fn workspace_root() -> PathBuf {
    let mut dir: &Path = &std::env::current_dir().expect("cwd");
    loop {
        let marker = dir.join("tools/openapi-gen");
        if marker.is_dir() {
            return dir.to_path_buf();
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => break,
        }
    }
    std::env::current_dir().expect("cwd")
}
