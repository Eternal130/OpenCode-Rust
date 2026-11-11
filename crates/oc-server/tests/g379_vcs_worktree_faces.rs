//! G-379.8 — the guest vcs + worktree read/op call faces over a real
//! git project: `ctx.vcs.branch/status/diff` and
//! `ctx.worktree.list/create/remove/refresh` route hostRequest →
//! supervisor → `EngineVcsPort`/`EngineWorktreePort` → the SHARED
//! handler cores the `/api/vcs/*` + `/api/worktree` routes ride (ref
//! promise adapter.ts:510-514 + 558-566).
//!
//! Hermetic (the `plugin_guest_wiring.rs` pattern): sandboxed
//! HOME/XDG, a git-repo project whose `opencode.json` loads the
//! g379faces fixture, bun-gated, no network.

use serde_json::json;

use oc_server::boot::{boot_and_serve, BootOptions};
use oc_server::lifecycle::ServeOptions;

mod support;

fn bun_available() -> bool {
    std::process::Command::new("bun")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success())
}

struct Sandbox {
    home: std::path::PathBuf,
    project: std::path::PathBuf,
    data_root: std::path::PathBuf,
}

fn sandbox(tag: &str) -> Sandbox {
    let root = std::path::PathBuf::from(format!(
        "/tmp/opencode/oc-server-g379-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    let project = root.join("project");
    std::fs::create_dir_all(&home).expect("home");
    std::fs::create_dir_all(&project).expect("project");
    let data_root = root.join("data");
    Sandbox { home, project, data_root }
}

fn git(dir: &std::path::Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .unwrap_or_else(|error| panic!("git {args:?}: {error}"));
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn redirect_env(home: &std::path::Path) {
    std::env::set_var("HOME", home);
    std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
    std::env::set_var("XDG_CACHE_HOME", home.join(".cache"));
    std::env::set_var("XDG_DATA_HOME", home.join(".local").join("share"));
    std::env::set_var("XDG_STATE_HOME", home.join(".local").join("state"));
    std::env::set_var("OPENCODE_CONFIG_DIR", home.join(".config").join("opencode-rust"));
    std::env::remove_var("OPENCODE_CONFIG");
    std::env::set_var("OPENCODE_CATALOG_FETCH", "0");
}

impl Sandbox {
    fn write_project_config(&self) {
        let plugin_path = oc_plugin_host::fixture_dir().join("g379faces.ts");
        assert!(plugin_path.is_file(), "fixture missing: {}", plugin_path.display());
        std::fs::write(
            self.project.join("opencode.json"),
            json!({ "plugins": [plugin_path.display().to_string()] }).to_string(),
        )
        .expect("config");
    }

    fn boot_options(&self) -> BootOptions {
        BootOptions {
            serve: ServeOptions::default(),
            password: None,
            cors_origins: Vec::new(),
            version: "2.0.24".to_owned(),
            data_root: self.data_root.clone(),
            home: self.home.clone(),
            config_location: Some(self.project.clone()),
            events_persist: None,
        }
    }
}

/// One command dispatch through the booted plugin host; the fixture
/// answers a JSON string — decode it.
async fn run(handle: &oc_server::lifecycle::ServeHandle, command: &str) -> serde_json::Value {
    let boot = handle.state.plugins.as_ref().expect("plugin boot attached");
    let outcome = boot
        .host
        .execute_command_in_session_async(command, None, "", std::time::Duration::from_secs(20))
        .await
        .unwrap_or_else(|error| panic!("{command}: {error}"));
    let text =
        outcome.as_str().unwrap_or_else(|| panic!("{command}: non-string {outcome}")).to_owned();
    serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("{command}: bad json ({error}): {text}"))
}

/// The vcs read faces: branch list (full + searched), status, and the
/// working/branch diff modes over a fixture git repo.
#[tokio::test]
async fn g379_guest_vcs_read_faces() {
    if !bun_available() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    let box_ = sandbox("vcs");
    // A fixture repo: a main commit, a feature branch, and uncommitted
    // changes on main.
    std::fs::write(box_.project.join("notes.md"), "seed\n").expect("seed");
    git(&box_.project, &["init", "--initial-branch=main"]);
    git(&box_.project, &["config", "user.email", "test@example.com"]);
    git(&box_.project, &["config", "user.name", "Test"]);
    git(&box_.project, &["add", "."]);
    git(&box_.project, &["commit", "-m", "seed"]);
    git(&box_.project, &["branch", "feature-lane5b"]);
    std::fs::write(box_.project.join("notes.md"), "seed\nplus an uncommitted line\n")
        .expect("edit");
    std::fs::write(box_.project.join("untracked.txt"), "new\n").expect("untracked");
    box_.write_project_config();
    redirect_env(&box_.home);

    let handle = boot_and_serve(&box_.boot_options()).await.expect("boot and serve");
    // Assemble the location once (the engine's project/vcs services +
    // the guest's ctx.location project id).
    let location = oc_core::location::LocationRef::new(box_.project.clone(), None);
    let _services = handle.state.engine.location(location).await.expect("location services");

    let result = run(&handle, "g379.vcs").await;
    let branches = &result["branches"];
    assert_eq!(branches["ok"], json!(true), "branch list ok: {branches}");
    let names = branches["value"].as_array().expect("branch array").clone();
    assert!(names.contains(&json!("main")), "main listed: {names:?}");
    assert!(names.contains(&json!("feature-lane5b")), "feature branch listed: {names:?}");

    let searched = &result["branchesSearch"];
    assert_eq!(searched["ok"], json!(true), "searched branch list ok: {searched}");
    assert_eq!(searched["value"], json!(["feature-lane5b"]), "the search filter applies");

    let status = &result["status"];
    assert_eq!(status["ok"], json!(true), "status ok: {status}");
    let rows = status["value"].as_array().expect("status array").clone();
    let files: Vec<String> =
        rows.iter().map(|row| row["file"].as_str().unwrap_or_default().to_owned()).collect();
    assert!(files.contains(&"notes.md".to_owned()), "modified file listed: {rows:?}");
    assert!(files.contains(&"untracked.txt".to_owned()), "untracked file listed: {rows:?}");

    let diff = &result["diff"];
    assert_eq!(diff["ok"], json!(true), "working diff ok: {diff}");
    let patches = diff["value"].as_array().expect("diff array").clone();
    let notes =
        patches.iter().find(|entry| entry["file"] == json!("notes.md")).expect("notes.md diff");
    assert!(notes["patch"].as_str().unwrap_or("").contains("+plus an uncommitted line"));

    let diff_branch = &result["diffBranch"];
    assert_eq!(diff_branch["ok"], json!(true), "branch diff ok: {diff_branch}");

    handle.shutdown_and_join().await.expect("shutdown");
}

/// The worktree op faces: list → create → list → remove → list, then a
/// host-made linked worktree reconciled by refresh.
#[tokio::test]
async fn g379_guest_worktree_roundtrip_and_refresh() {
    if !bun_available() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    let box_ = sandbox("worktree");
    std::fs::write(box_.project.join("README.md"), "seed\n").expect("seed");
    git(&box_.project, &["init", "--initial-branch=main"]);
    git(&box_.project, &["config", "user.email", "test@example.com"]);
    git(&box_.project, &["config", "user.name", "Test"]);
    git(&box_.project, &["add", "."]);
    git(&box_.project, &["commit", "-m", "seed"]);
    box_.write_project_config();
    redirect_env(&box_.home);

    let parent = box_.data_root.parent().unwrap().join("wt-parent");
    std::fs::create_dir_all(&parent).expect("worktree parent");
    std::env::set_var("OC_G379_PARENT", &parent);

    let handle = boot_and_serve(&box_.boot_options()).await.expect("boot and serve");
    let location = oc_core::location::LocationRef::new(box_.project.clone(), None);
    let _services = handle.state.engine.location(location).await.expect("location services");

    let roundtrip = run(&handle, "g379.worktree.roundtrip").await;
    let project_id = roundtrip["projectID"].as_str().unwrap_or_default().to_owned();
    assert!(!project_id.is_empty(), "the guest sees the location's project id");

    // list before: the canonical checkout row.
    let before = &roundtrip["listBefore"];
    assert_eq!(before["ok"], json!(true), "list ok: {before}");
    assert!(
        before["value"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["directory"] == json!(box_.project.display().to_string())),
        "the canonical checkout is a saved row: {before}"
    );

    // create: a real linked worktree under the requested parent.
    let created = &roundtrip["created"];
    assert_eq!(created["ok"], json!(true), "create ok: {created}");
    let directory = created["value"]["directory"].as_str().expect("created directory").to_owned();
    assert_eq!(directory, parent.join("lane5b").display().to_string());

    // list after create includes it with the git strategy.
    let after_create = &roundtrip["listAfterCreate"];
    let row = after_create["value"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["directory"] == json!(directory))
        .unwrap_or_else(|| panic!("created row listed: {after_create}"));
    assert_eq!(row["strategy"], json!("git"));

    // remove then list: gone (the roundtrip removed it in-command; the
    // git worktree remove also deletes the on-disk directory).
    assert_eq!(roundtrip["removed"]["ok"], json!(true), "remove ok: {roundtrip}");
    assert!(
        !roundtrip["listAfterRemove"]["value"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["directory"] == json!(directory)),
        "the row is gone after remove"
    );
    assert!(
        !std::path::Path::new(&directory).is_dir(),
        "the git worktree remove deleted the directory"
    );

    // refresh: a HOST-made linked worktree (never registered) is
    // discovered and reconciled into the inventory.
    let stray = box_.data_root.parent().unwrap().join("wt-stray");
    git(&box_.project, &["worktree", "add", "--detach", stray.to_str().unwrap(), "HEAD"]);
    let refresh = run(&handle, "g379.worktree.refresh").await;
    let after = &refresh["after"];
    assert_eq!(after["ok"], json!(true), "post-refresh list ok: {after}");
    assert!(
        after["value"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["directory"] == json!(stray.display().to_string())),
        "the host-made linked worktree is discovered: {after}"
    );

    handle.shutdown_and_join().await.expect("shutdown");
}
