//! RF-02 — the plugin UPDATE path end-to-end over PATH-shimmed
//! package managers (the RA-02 shim pattern): outdated detection
//! against a fake registry, a real new-generation install through a
//! shimmed installer, the check-cache clear, and the handler-shaped
//! `update` flow.
//!
//! Hermetic: a tmp sandbox under /tmp/opencode, shims for `npm view`
//! and `bun install`, no network.

use std::path::{Path, PathBuf};

use oc_plugin_host::{install_root, update, update_cache, InstallOptions, UpdateCache};

fn sandbox(tag: &str) -> PathBuf {
    let dir =
        PathBuf::from(format!("/tmp/opencode/ocph-update-lane-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("sandbox dir");
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

/// A fake `npm`: `view <pkg> version` echoes the current contents of
/// `registry-version.txt` (mutable — the test flips the registry).
fn npm_shim(dir: &Path) -> PathBuf {
    std::fs::write(dir.join("registry-version.txt"), "1.0.0").expect("registry seed");
    make_executable(
        &dir.join("npm"),
        &format!(
            "#!/bin/sh\n[ \"$1\" = \"view\" ] && cat \"{}/registry-version.txt\" && exit 0\nexit 1\n",
            dir.display()
        ),
    );
    dir.join("npm")
}

/// A fake `bun`: `install` materializes `node_modules/<pkg>` from the
/// `<pkg>@<version>` cache key two levels up (the manifest's single
/// dependency, the exact shape `write_install_manifest` writes).
fn bun_shim(dir: &Path) -> PathBuf {
    make_executable(
        &dir.join("bun"),
        "#!/bin/sh\n[ \"$1\" = \"install\" ] || exit 1\nkey=$(basename \"$(dirname \"$PWD\")\")\nname=${key%@*}\nver=${key##*@}\npkgdir=\"$PWD/node_modules/$name\"\nmkdir -p \"$pkgdir\"\nprintf '{\"name\":\"%s\",\"version\":\"%s\",\"main\":\"plugin.js\"}\\n' \"$name\" \"$ver\" > \"$pkgdir/package.json\"\nprintf 'export default {}\\n' > \"$pkgdir/plugin.js\"\nexit 0\n",
    );
    dir.join("bun")
}

fn stage(root: &Path, package: &str, version: &str, stamp: &str) {
    let pkg = root
        .join(format!("{package}@{version}"))
        .join(stamp)
        .join("node_modules")
        .join(package);
    std::fs::create_dir_all(&pkg).expect("stage dir");
    std::fs::write(
        pkg.join("package.json"),
        format!("{{\"name\":\"{package}\",\"version\":\"{version}\",\"main\":\"plugin.js\"}}"),
    )
    .expect("stage manifest");
    std::fs::write(pkg.join("plugin.js"), "export default {}").expect("stage entry");
}

fn stamp_dirs(root: &Path, package: &str, version: &str) -> Vec<String> {
    let key = root.join(format!("{package}@{version}"));
    let mut stamps: Vec<String> = std::fs::read_dir(&key)
        .expect("read key")
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.file_name().into_string().unwrap())
        .collect();
    stamps.sort();
    stamps
}

#[test]
fn update_detects_installs_and_rechecks() {
    let box_ = sandbox("flow");
    let npm = npm_shim(&box_);
    let bun = bun_shim(&box_);
    // A dedicated cache root (never the developer's real one).
    let root = box_.join("npm-cache");
    std::fs::create_dir_all(&root).expect("cache root");
    stage(&root, "demo-plugin", "1.0.0", "1000");

    let options = InstallOptions {
        root: Some(root.clone()),
        npm: Some(npm),
        bun: Some(bun),
        ..InstallOptions::default()
    };

    // 1.0.0 installed, registry says 1.0.0 → not outdated.
    let cache = UpdateCache::new();
    assert!(!cache.check("demo-plugin", &root, &options, true));

    // The registry moves to 2.0.0 → outdated.
    std::fs::write(box_.join("registry-version.txt"), "2.0.0").expect("bump registry");
    assert!(cache.check("demo-plugin", &root, &options, true));

    // The update installs the new generation (a REAL install through
    // the shimmed installer, not the reuse path).
    let installed = update("demo-plugin", &root, &cache, &options).expect("update runs");
    assert!(!installed.reused, "a fresh generation was installed");
    assert_eq!(installed.version, "2.0.0");
    assert!(installed.root.display().to_string().contains("demo-plugin@2.0.0"));
    assert_ne!(
        installed.root.file_name().and_then(|n| n.to_str()),
        Some("1000"),
        "the new generation is a new timestamp dir"
    );

    // The cached check was cleared: the fresh probe compares the NEW
    // install against the registry → not outdated.
    assert!(!cache.check("demo-plugin", &root, &options, true));

    // Both generations survive (keep-2 floor).
    assert_eq!(stamp_dirs(&root, "demo-plugin", "1.0.0").len(), 1);
    assert_eq!(stamp_dirs(&root, "demo-plugin", "2.0.0").len(), 1);
}

#[test]
fn update_sweeps_stale_generations_after_the_update() {
    // F1 regression: `update()` must run the GC sweep (npm.ts:396) —
    // stale generations (> 7 d, beyond the keep-2 floor) disappear as
    // a side effect of the update itself, not only via a manual
    // `collect_stale` call.
    let box_ = sandbox("gc-after-update");
    let npm = npm_shim(&box_);
    let bun = bun_shim(&box_);
    let root = box_.join("npm-cache");
    std::fs::create_dir_all(&root).expect("cache root");

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let day = 24 * 60 * 60 * 1_000u128;
    // The installed 1.0.0 line: two fresh generations (the keep-2
    // floor protects them whatever their age) plus two stale ones past
    // the 7-day retention.
    stage(&root, "demo-plugin", "1.0.0", &now.to_string());
    stage(&root, "demo-plugin", "1.0.0", &(now - day).to_string());
    stage(&root, "demo-plugin", "1.0.0", &(now - 8 * day).to_string());
    stage(&root, "demo-plugin", "1.0.0", &(now - 9 * day).to_string());

    let options = InstallOptions {
        root: Some(root.clone()),
        npm: Some(npm),
        bun: Some(bun),
        ..InstallOptions::default()
    };
    std::fs::write(box_.join("registry-version.txt"), "2.0.0").expect("bump registry");
    let cache = UpdateCache::new();
    let installed = update("demo-plugin", &root, &cache, &options).expect("update runs");
    assert_eq!(installed.version, "2.0.0");

    // GC ran as part of the update: the stale 1.0.0 generations (past
    // retention AND beyond the keep-2 floor) are gone; the two newest
    // survive unconditionally.
    let stamps = stamp_dirs(&root, "demo-plugin", "1.0.0");
    assert!(
        !stamps.contains(&(now - 8 * day).to_string()),
        "stale generation swept by update(): {stamps:?}"
    );
    assert!(
        !stamps.contains(&(now - 9 * day).to_string()),
        "stale generation swept by update(): {stamps:?}"
    );
    assert!(stamps.contains(&now.to_string()), "newest generation kept: {stamps:?}");
    assert!(
        stamps.contains(&(now - day).to_string()),
        "keep-2 floor protects the second-newest: {stamps:?}"
    );
}

#[test]
fn update_failure_surfaces_the_installer_error() {
    let box_ = sandbox("fail");
    let npm = npm_shim(&box_);
    // A bun shim that always fails → the install leg errors.
    make_executable(&box_.join("bun-fail"), "#!/bin/sh\nexit 3\n");
    let root = box_.join("npm-cache");
    std::fs::create_dir_all(&root).expect("cache root");
    stage(&root, "demo-plugin", "1.0.0", "1000");
    std::fs::write(box_.join("registry-version.txt"), "2.0.0").expect("bump registry");

    let options = InstallOptions {
        root: Some(root.clone()),
        npm: Some(npm),
        bun: Some(box_.join("bun-fail")),
        ..InstallOptions::default()
    };
    let cache = UpdateCache::new();
    let error = update("demo-plugin", &root, &cache, &options).expect_err("install fails");
    assert!(error.contains("failed"), "installer failure surfaces: {error}");
    // The 1.0.0 generation is untouched.
    assert_eq!(stamp_dirs(&root, "demo-plugin", "1.0.0").len(), 1);
}

#[test]
fn global_cache_is_shared_per_process() {
    // The process-global cache backs the HTTP handler (the ref's global
    // PluginUpdate service node).
    let _ = update_cache();
    let _ = install_root();
}
