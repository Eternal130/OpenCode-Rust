//! G-332 (host side): the plugin markdown code-block renderer
//! registration chain — `ctx.markdown.registerCodeBlockRenderer`
//! lands in the registration snapshot, the blank-language and
//! intra-plugin-duplicate error texts throw ref-exact in the guest,
//! the dispose closure unregisters, and the bounded facade
//! `render_markdown_block` materializes the plugin's flattened rows.
//!
//! Hermetic: /tmp/opencode sandbox, no network (marker files are the
//! observable effects).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

fn sandbox() -> PathBuf {
    let dir = PathBuf::from(format!("/tmp/opencode/oc-md-renderers-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("sandbox");
    std::fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mdrenderer.ts"),
        dir.join("mdrenderer.ts"),
    )
    .expect("fixture copy");
    dir
}

fn boot() -> Option<(Arc<oc_plugin_host::PluginHost>, PathBuf)> {
    let sandbox = sandbox();
    let config = oc_plugin_host::PluginHostConfig {
        env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string())],
        ..oc_plugin_host::PluginHostConfig::default()
    };
    let host = oc_plugin_host::PluginHost::detect()
        .start_detached_with(config, &[sandbox.join("mdrenderer.ts")]);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return None;
    }
    assert!(host.wait_ready(Duration::from_secs(20)), "host ready: {:?}", host.start_error());
    Some((Arc::new(host), sandbox))
}

fn wait_for(timeout: Duration, mut probe: impl FnMut() -> bool) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if probe() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

/// The registration snapshot carries exactly one renderer (the
/// duplicate throws, the dispose unregisters, the re-register lands),
/// keyed by the RAW language.
#[test]
fn snapshot_carries_markdown_renderer() {
    let Some((host, sandbox)) = boot() else { return };
    assert!(
        wait_for(Duration::from_secs(10), || host
            .registrations()
            .markdown_renderers
            .iter()
            .any(|renderer| renderer.plugin_id == "mdrenderer"
                && renderer.language == "wormlang")),
        "the renderer registered"
    );
    // The dispose unregistered the FIRST registration; the
    // re-registered one is the single entry.
    assert_eq!(host.registrations().markdown_renderers.len(), 1);

    host.shutdown();
    let _ = sandbox;
}

/// The ref-exact error texts surface as guest throws (marker files).
#[test]
fn blank_and_duplicate_errors_are_ref_exact() {
    let Some((host, sandbox)) = boot() else { return };
    assert!(
        wait_for(Duration::from_secs(10), || sandbox.join("setup-done.txt").exists()),
        "setup completes"
    );
    let blank = std::fs::read_to_string(sandbox.join("md-blank-error.txt")).expect("blank marker");
    assert_eq!(blank, "Markdown code-block language is required");
    let dup = std::fs::read_to_string(sandbox.join("md-dup-error.txt")).expect("dup marker");
    assert_eq!(dup, "Markdown code-block renderer already registered: wormlang");

    host.shutdown();
}

/// The bounded facade materializes the plugin's flattened rows (every
/// flattened line is a row — the render inputs echo back).
#[test]
fn render_markdown_block_materializes_rows() {
    let Some((host, sandbox)) = boot() else { return };
    assert!(
        wait_for(Duration::from_secs(10), || host
            .registrations()
            .markdown_renderers
            .iter()
            .any(|renderer| renderer.language == "wormlang")),
        "the renderer registered"
    );
    let spec = host
        .render_markdown_block("mdrenderer", "wormlang", "abc", 40, Duration::from_secs(5))
        .expect("render materializes");
    assert_eq!(spec.rows, vec!["worm: wormlang", "len: 3", "up: ABC"]);
    assert!(!spec.clickable.iter().any(|clicked| *clicked));

    // The width budget flattens against the caller's span: a narrow
    // budget keeps the same rows here (none exceed it).
    let narrow = host
        .render_markdown_block("mdrenderer", "wormlang", "xyz", 40, Duration::from_secs(5))
        .expect("narrow render materializes");
    assert_eq!(narrow.rows, vec!["worm: wormlang", "len: 3", "up: XYZ"]);

    // Unknown language → the bounded miss replies an error.
    assert!(host
        .render_markdown_block("mdrenderer", "nosuchlang", "x", 40, Duration::from_secs(5))
        .is_err());

    host.shutdown();
    let _ = sandbox;
}
