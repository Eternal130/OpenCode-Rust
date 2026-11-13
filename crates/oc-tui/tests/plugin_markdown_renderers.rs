//! G-332 (TUI side): the plugin markdown code-block renderer dispatch
//! table + content cache — the identity gate (empty table ⇒ every
//! existing byte identical), the table build (raw language →
//! normalized filetype, last-wins), and the full boot chain (fixture
//! renderer registered → cache MISS renders the default path → the
//! bounded off-loop render lands → the next render emits the plugin
//! rows).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use oc_tui::components::message::markdown::{render_markdown, MarkdownStyles};
use oc_tui::plugin_markdown;
use oc_tui::reactive::Runtime;
use oc_tui::store::Store;
use oc_tui::theme::ThemeState;

fn sandbox() -> PathBuf {
    let dir = PathBuf::from(format!("/tmp/opencode/oc-tui-md-renderer-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("sandbox");
    std::fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../oc-plugin-host/tests/fixtures/mdrenderer.ts"),
        dir.join("mdrenderer.ts"),
    )
    .expect("fixture copy");
    dir
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

/// The table build: raw languages normalize
/// (`info_string_to_filetype`), the LAST registration per normalized
/// filetype wins, and an empty table misses everywhere.
#[test]
fn table_normalizes_and_last_wins() {
    plugin_markdown::reset_for_test();
    plugin_markdown::install_table(
        None,
        &[
            ("plugin-a".to_owned(), "wormlang".to_owned()),
            ("plugin-a".to_owned(), "WormLang extra".to_owned()),
            ("plugin-b".to_owned(), "ts".to_owned()),
        ],
    );
    // Both raw spellings normalize to `wormlang`; the later
    // registration (raw key `WormLang extra`) wins.
    let hit = plugin_markdown::lookup("wormlang").expect("wormlang resolves");
    assert_eq!(hit.0, "plugin-a");
    assert_eq!(hit.1, "WormLang extra");
    // The alias table: `ts` normalizes to `typescript`.
    let ts = plugin_markdown::lookup("typescript").expect("ts normalizes");
    assert_eq!(ts.0, "plugin-b");
    // Misses stay misses.
    assert!(plugin_markdown::lookup("rust").is_none());
    plugin_markdown::reset_for_test();
    assert!(plugin_markdown::lookup("wormlang").is_none());
}

/// The identity gate: a fence whose filetype has NO registered
/// renderer renders byte-identically to the pre-G-332 plain path —
/// and even a REGISTERED filetype renders the default path until the
/// cache fills (no host ⇒ the miss never resolves).
#[test]
fn identity_gate_empty_table_and_miss_render_default() {
    plugin_markdown::reset_for_test();
    let styles = MarkdownStyles::from_theme(&ThemeState::v2_dark().styles());
    let source = "```wormlang\nlet x = \"s\";\n```";
    // Empty table: the plain unknown-fence path, byte-identical.
    let plain = render_markdown(source, 40, &styles);
    let empty_info = render_markdown("```\nlet x = \"s\";\n```", 40, &styles);
    assert_eq!(plain, empty_info, "unknown filetype renders the plain path");

    // Registered table WITHOUT a host: the cache miss fires (a no-op
    // spawn — no host) and the default path renders this frame.
    plugin_markdown::install_table(None, &[("plugin-a".to_owned(), "wormlang".to_owned())]);
    let miss = render_markdown(source, 40, &styles);
    assert_eq!(miss, plain, "cache miss renders the default path byte-identically");
    plugin_markdown::reset_for_test();
}

/// The full chain: the mdrenderer fixture boots through the plugin
/// runtime, the attach-built table picks up its renderer, the first
/// render falls back (cache miss), the bounded off-loop render lands
/// (`rendered_version` bumps), and the next render emits the plugin's
/// rows as base-styled clipped spans.
#[test]
fn boot_chain_caches_plugin_rows() {
    plugin_markdown::reset_for_test();
    let sandbox = sandbox();
    let config = oc_plugin_host::PluginHostConfig {
        env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string())],
        ..oc_plugin_host::PluginHostConfig::default()
    };
    let host = Arc::new(
        oc_plugin_host::PluginHost::detect()
            .start_detached_with(config, &[sandbox.join("mdrenderer.ts")]),
    );
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(20)), "host ready: {:?}", host.start_error());
    let booted = oc_tui::plugin_boot::BootedPluginHost {
        host,
        resolution: oc_plugin_host::resolve(&[], None),
        ready: true,
    };
    let runtime = oc_tui::plugin_boot::TuiPluginRuntime::new(booted);
    let store = Store::new(Runtime::new());
    runtime.attach(&store);
    assert!(
        wait_for(Duration::from_secs(10), || plugin_markdown::lookup("wormlang").is_some()),
        "attach built the dispatch table"
    );

    let styles = MarkdownStyles::from_theme(&ThemeState::v2_dark().styles());
    let source = "```wormlang\nabc\n```";
    // The miss renders the default plain path first.
    let miss = render_markdown(source, 40, &styles);
    assert_eq!(miss.len(), 1);
    assert_eq!(miss[0].text(), "abc");

    // The off-loop render lands: the version bumps and the rows cache.
    let before = plugin_markdown::rendered_version();
    assert!(
        wait_for(Duration::from_secs(10), || { plugin_markdown::rendered_version() > before }),
        "the bounded render landed"
    );
    let hit = render_markdown(source, 40, &styles);
    let texts: Vec<String> =
        hit.iter().map(oc_tui::components::message::MarkdownLine::text).collect();
    assert_eq!(texts, vec!["worm: wormlang", "len: 3", "up: ABC"]);

    runtime.shutdown();
    plugin_markdown::reset_for_test();
}
