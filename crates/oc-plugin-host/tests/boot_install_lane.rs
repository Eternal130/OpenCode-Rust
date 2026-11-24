//! G-313 (overturned 2026-11-24, ref parity) — the BOOT auto-install
//! leg (`resolve_with_boot_install`): resolve-miss → install →
//! re-resolve hits; install failures splice onto the failure row with
//! ONE attempt per spec; the named git-spec install chain
//! (`pkg@git+file://…`, the same shape as
//! `superpowers@git+https://…`) runs end-to-end over a REAL bun + a
//! local file:// git repository (no network; skipped when bun or git
//! is unavailable on the box).
//!
//! Hermetic otherwise: a tmp sandbox under /tmp/opencode, shims for
//! `bun install` (the plugin_update_lane pattern), no network, no
//! writes to the user's real `~/.cache/opencode-rust/npm`.

use std::path::{Path, PathBuf};

use oc_plugin_host::{resolve_with_boot_install, InstallOptions, PluginOp};

fn sandbox(tag: &str) -> PathBuf {
    let dir = PathBuf::from(format!("/tmp/opencode/ocph-boot-install-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir.join("shims")).expect("sandbox dir");
    dir
}

fn make_executable(path: &Path, body: &str) {
    std::fs::write(path, body).expect("write shim");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
}

/// A fake `bun` (the plugin_update_lane pattern): `install`
/// materializes `node_modules/<pkg>` from the `<pkg>@<version>` cache
/// key two levels up, and logs every invocation for the once-only
/// assertion.
fn bun_shim(dir: &Path) -> PathBuf {
    let shim = dir.join("bun");
    make_executable(
        &shim,
        &format!(
            "#!/bin/sh\necho \"$*\" >> \"{}/invocations.log\"\n[ \"$1\" = \"install\" ] || exit 1\n\
             key=$(basename \"$(dirname \"$PWD\")\")\nname=${{key%@*}}\nver=${{key##*@}}\n\
             pkgdir=\"$PWD/node_modules/$name\"\nmkdir -p \"$pkgdir\"\n\
             printf '{{\"name\":\"%s\",\"version\":\"%s\",\"main\":\"plugin.js\"}}\\n' \"$name\" \
             \"$ver\" > \"$pkgdir/package.json\"\n\
             printf 'export default {{ id: \"%s\" }}\\n' \"$name\" > \"$pkgdir/plugin.js\"\nexit 0\n",
            dir.display()
        ),
    );
    shim
}

fn ops(spec: &str) -> Vec<PluginOp> {
    vec![PluginOp::Add { target: spec.to_owned(), options: serde_json::Value::Null }]
}

#[test]
fn boot_resolve_miss_installs_then_hits() {
    let dir = sandbox("miss-install");
    let root = dir.join("npm-cache");
    std::fs::create_dir_all(&root).expect("cache root");
    let options = InstallOptions {
        root: Some(root.clone()),
        bun: Some(bun_shim(&dir)),
        ..InstallOptions::default()
    };

    // First resolve (offline) misses.
    let cold = oc_plugin_host::resolve(&ops("demo-plugin@1.0.0"), Some(root.clone()));
    assert!(cold.plugins.is_empty(), "cold resolve should miss");
    assert_eq!(cold.failures.len(), 1);

    // The boot leg installs and the re-resolve activates the plugin.
    let resolution = resolve_with_boot_install(&ops("demo-plugin@1.0.0"), Some(root), &options);
    assert!(
        resolution.failures.is_empty(),
        "boot install should clear the failure: {:?}",
        resolution.failures
    );
    assert_eq!(resolution.plugins.len(), 1);
    assert_eq!(resolution.plugins[0].target, "demo-plugin@1.0.0");
    assert!(
        resolution.plugins[0].entry.ends_with("node_modules/demo-plugin/plugin.js"),
        "entry should hit the installed generation: {}",
        resolution.plugins[0].entry.display()
    );
}

#[test]
fn boot_install_failure_splices_error_and_attempts_once() {
    let dir = sandbox("fail-once");
    let root = dir.join("npm-cache");
    std::fs::create_dir_all(&root).expect("cache root");
    // A bun that always fails.
    let shim = dir.join("bun");
    make_executable(
        &shim,
        &format!(
            "#!/bin/sh\necho \"$*\" >> \"{}/invocations.log\"\necho 'shim install exploded' >&2\n\
             exit 1\n",
            dir.display()
        ),
    );
    let options =
        InstallOptions { root: Some(root.clone()), bun: Some(shim), ..InstallOptions::default() };

    let resolution = resolve_with_boot_install(&ops("broken-plugin@2.0.0"), Some(root), &options);
    assert_eq!(resolution.plugins.len(), 0, "nothing activates on install failure");
    assert_eq!(resolution.failures.len(), 1, "the one failed spec surfaces once");
    let (target, error) = &resolution.failures[0];
    assert_eq!(target, "broken-plugin@2.0.0");
    assert!(
        error.contains("boot auto-install failed") && error.contains("shim install exploded"),
        "failure row should splice the install error: {error}"
    );
    // Exactly ONE install attempt for the spec (no retry storm).
    let invocations =
        std::fs::read_to_string(dir.join("invocations.log")).expect("invocations log");
    assert_eq!(invocations.lines().count(), 1, "one attempt per spec per boot");
}

#[test]
fn boot_install_local_path_failures_are_not_installed() {
    let dir = sandbox("local-not-installed");
    let root = dir.join("npm-cache");
    std::fs::create_dir_all(&root).expect("cache root");
    let shim = dir.join("bun");
    make_executable(&shim, "#!/bin/sh\nexit 0\n");
    let options =
        InstallOptions { root: Some(root), bun: Some(shim), ..InstallOptions::default() };

    // A local spec that does not exist: NOT an "is not installed"
    // miss — the boot leg must not hand it to the installer.
    let resolution =
        resolve_with_boot_install(&ops("./no-such-local-plugin"), Some(dir.join("npm-cache")), &options);
    assert_eq!(resolution.failures.len(), 1);
    assert!(
        !resolution.failures[0].1.contains("auto-install"),
        "local path errors stay untouched: {:?}",
        resolution.failures[0].1
    );
    assert!(
        !dir.join("invocations.log").exists(),
        "the installer must not run for local-path failures"
    );
}

/// The named git-spec install chain end-to-end over a git-aware bun
/// SHIM and a local `file://` git repository (the
/// `superpowers@git+https://github.com/…` shape, hermetic): manifest
/// write → install (git clone) → staging rename → resolve hit on the
/// SAME `<pkg>@<git-url>` cache key (the nested-directory layout both
/// `install` and `resolve_one` agree on).
///
/// NOTE bun 1.3.10 cannot resolve `git+file://` refs itself (probed
/// live: "no commit matching … found" for every ref form while
/// `git+https` works — the live superpowers install in the lane
/// report), so the shim performs the clone; everything AFTER the
/// installer (keys, staging, rename, entry resolution) is the real
/// production code under test.
#[test]
fn named_git_spec_full_chain_local_file_repo() {
    if !command_on_path("git") {
        eprintln!("git unavailable; skipping");
        return;
    }
    if !command_on_path("python3") {
        eprintln!("python3 unavailable; skipping");
        return;
    }
    let dir = sandbox("git-spec");
    let root = dir.join("npm-cache");
    std::fs::create_dir_all(&root).expect("cache root");

    // The "remote": a local git repository publishing a plugin package.
    let repo = dir.join("remote-plugin");
    std::fs::create_dir_all(&repo).expect("repo dir");
    std::fs::write(
        repo.join("package.json"),
        r#"{ "name": "git-plugin-demo", "version": "1.4.2", "main": "plugin.js" }"#,
    )
    .expect("manifest");
    std::fs::write(repo.join("plugin.js"), "export default { id: 'git-plugin-demo' }\n")
        .expect("entry");
    assert_run("git", &["init", "-q", "-b", "main"], &repo);
    assert_run("git", &["add", "."], &repo);
    assert_run(
        "git",
        &["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qm", "init"],
        &repo,
    );

    // The git-aware bun shim: `bun install` clones each `git+…`
    // dependency into node_modules (the git+file:// transport bun
    // itself cannot resolve — see the note above). The logic lives in
    // a sibling python file (shell-quoting-proof).
    let installer_py = dir.join("install.py");
    std::fs::write(
        &installer_py,
        r#"
import json, os, subprocess, shutil
deps = json.load(open("package.json"))["dependencies"]
for name, spec in deps.items():
    if spec.startswith("git+"):
        target = os.path.join("node_modules", name)
        subprocess.run(["git", "clone", "-q", spec[4:], target], check=True)
        shutil.rmtree(os.path.join(target, ".git"), ignore_errors=True)
"#,
    )
    .expect("write install.py");
    let shim = dir.join("bun");
    make_executable(
        &shim,
        &format!(
            "#!/bin/sh\n[ \"$1\" = \"install\" ] || exit 1\npython3 \"{}\"\nexit 0\n",
            installer_py.display()
        ),
    );

    let spec = format!("git-plugin-demo@git+file://{}", repo.display());
    let options = InstallOptions {
        root: Some(root.clone()),
        bun: Some(shim),
        ..InstallOptions::default()
    };

    let cold = oc_plugin_host::resolve(&ops(&spec), Some(root.clone()));
    assert!(cold.plugins.is_empty(), "cold resolve should miss");

    let resolution = resolve_with_boot_install(&ops(&spec), Some(root), &options);
    assert!(
        resolution.failures.is_empty(),
        "named git spec should install+resolve: {:?}",
        resolution.failures
    );
    assert_eq!(resolution.plugins.len(), 1);
    assert_eq!(resolution.plugins[0].target, spec);
    assert!(
        resolution.plugins[0].entry.ends_with("node_modules/git-plugin-demo/plugin.js"),
        "entry should hit the installed generation: {}",
        resolution.plugins[0].entry.display()
    );
}

/// The git-availability guard: a git-sourced spec with `git` absent
/// from PATH fails with the dedicated message (not an opaque clone
/// error). The PATH is narrowed to a shim directory holding only bun.
#[test]
fn git_spec_without_git_names_the_missing_tool() {
    let dir = sandbox("no-git");
    let root = dir.join("npm-cache");
    std::fs::create_dir_all(&root).expect("cache root");
    let shim = dir.join("shims-only");
    std::fs::create_dir_all(&shim).expect("shim dir");
    make_executable(&shim.join("bun"), "#!/bin/sh\nexit 1\n");

    let repo = dir.join("remote-plugin");
    std::fs::create_dir_all(&repo).expect("repo dir");
    let spec = format!("git-plugin-demo@git+file://{}", repo.display());

    let old_path = std::env::var_os("PATH").expect("PATH");
    // Narrow PATH to a directory with bun but NO git.
    std::env::set_var("PATH", &shim);
    let result = oc_plugin_host::install(
        &spec,
        &InstallOptions {
            root: Some(root),
            bun: Some(shim.join("bun")),
            ..InstallOptions::default()
        },
    );
    std::env::set_var("PATH", old_path);

    let error = result.expect_err("install must fail without git");
    assert!(
        error.contains("git executable not found"),
        "the dedicated git-missing message must surface: {error}"
    );
}

fn command_on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .map(|path| {
            std::env::split_paths(&path).any(|dir| dir.join(program).is_file())
        })
        .unwrap_or(false)
}

fn assert_run(program: &str, args: &[&str], cwd: &Path) {
    let status = std::process::Command::new(program)
        .args(args)
        .current_dir(cwd)
        .status()
        .expect("spawn git");
    assert!(status.success(), "{program} {args:?} failed in {}", cwd.display());
}

