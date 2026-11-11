//! G-350 — vcs.init plugin provider dispatch (ref vcs.ts:178-191:
//! `initialize` resolves the provider in the registry and calls its
//! `init` closure). The guest's `ctx.vcs.transform` registers the
//! provider with its closure; the host's registration row carries a
//! `has_init` flag (the 501 "unsupported" seam), and the "vcs"/"init"
//! hook dispatch reaches the closure in the guest.

mod common;

use std::time::Duration;

use serde_json::json;

#[test]
fn vcs_provider_init_dispatches_to_the_guest() {
    let sandbox = common::sandbox("g350vcs");
    let entry = common::copy_fixture(&sandbox, "fakevcs.ts");
    let host = common::host_with(&[entry], &sandbox);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)));

    // Registration rows carry the has_init flag (novcs → 501 seam).
    let rows = host.registrations().vcs_providers.clone();
    assert!(
        rows.iter().any(|row| row.id == "fakevcs" && row.has_init),
        "fakevcs registered with init: {rows:?}"
    );
    assert!(
        rows.iter().any(|row| row.id == "novcs" && !row.has_init),
        "novcs registered without init: {rows:?}"
    );

    let work = sandbox.join("proj");
    std::fs::create_dir_all(&work).expect("proj dir");
    let worktree = work.display().to_string();
    let event = json!({
        "provider": "fakevcs",
        "input": { "worktree": worktree, "directory": worktree, "canonical": worktree },
    });

    // The dispatch invokes the closure: the minimal repo + the scope
    // marker land on disk.
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("rt");
    rt.block_on(host.dispatch_hook_async("fakevcs", "vcs", "init", event))
        .expect("init dispatch succeeds");
    assert!(work.join(".git").join("HEAD").exists(), "init ran in the worktree");
    let marker = common::read(&sandbox.join("fakevcs-init.json"));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&marker).expect("marker json"),
        json!({ "worktree": worktree, "directory": worktree, "canonical": worktree }),
        "the init scope crossed the boundary: {marker}"
    );

    // A provider mismatch is a no-op (the handler filters on
    // event.provider).
    let other = sandbox.join("other");
    std::fs::create_dir_all(&other).expect("other dir");
    let wrong = json!({
        "provider": "not-registered",
        "input": { "worktree": other.display().to_string(), "directory": "", "canonical": "" },
    });
    rt.block_on(host.dispatch_hook_async("fakevcs", "vcs", "init", wrong))
        .expect("mismatched provider dispatch is a no-op");
    assert!(!other.join(".git").exists(), "no init for a mismatched provider");
}
