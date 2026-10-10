//! Context generalization (wave 4, deliverable 4): `ctx.generate.text`
//! — the one domain the flagship exercises (probes at setup, logs
//! `available: false`, degrades) that wave 2 left unwired. The
//! flagship-driven rule holds: nothing speculative joins the Context —
//! this is the flagged gap in the w9w2 domain list.

mod common;

use std::time::Duration;

use oc_plugin_host::PluginHostConfig;
use serde_json::json;

#[test]
fn generate_text_round_trips_through_the_engine_seam() {
    let sandbox = common::sandbox("generate");
    let entry = common::copy_fixture(&sandbox, "generateprobe.ts");
    let recording = std::sync::Arc::new(common::RecordingSessionPort::default());
    let config = PluginHostConfig {
        env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string())],
        session_port: Some(recording.clone()),
        ..PluginHostConfig::default()
    };
    let host = oc_plugin_host::PluginHost::detect().start_detached_with(config, &[entry]);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)));

    let result = host.execute_command("genprobe.ask", None, Duration::from_secs(5));
    assert!(result.is_ok(), "the generation command completed: {result:?}");

    // The engine seam saw the one-off request and its answer flowed
    // back into the plugin.
    let generates = recording.generates.lock().unwrap();
    assert_eq!(generates.len(), 1, "exactly one generate.text call");
    assert_eq!(generates[0].0, "summarize: wave 4");
    assert_eq!(generates[0].1["providerID"], json!("zhipuai"));
    drop(generates);
    assert_eq!(
        common::read(&sandbox.join("generate-out.txt")),
        "summary of: summarize: wave 4",
        "the plugin received the generated text"
    );
    host.shutdown();
}
