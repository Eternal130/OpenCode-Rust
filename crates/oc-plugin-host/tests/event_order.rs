//! Event-tap ordering (wave 4, deliverable 3): the plugin sees the
//! EXACT deduplicated, ordered stream the store applied — the tap
//! publishes post-ingest (the bus's persist-then-publish listener), and
//! the G11-F1 first-connect window (an event landing in BOTH a snapshot
//! replay and the live stream) cannot double-deliver.
//!
//! The "replaying server" is the real oc-core bus: `Bus::log(after)`
//! replays the durable history exactly like the SSE Last-Event-ID
//! resync a reconnecting consumer performs.

mod common;

use std::sync::Arc;
use std::time::Duration;

use oc_plugin_host::{PluginHost, PluginHostConfig, TapEvent};

fn renamed_def() -> &'static oc_core::bus::EventDefinition {
    oc_core::bus::by_type_latest("session.renamed").expect("session.renamed registered")
}

#[test]
#[allow(clippy::too_many_lines)] // one sequential scenario: bus → tap → guest markers
fn plugin_observes_the_applied_stream_one_to_one() {
    let sandbox = common::sandbox("event-order");
    let entry = common::copy_fixture(&sandbox, "taprecorder.ts");
    let config = PluginHostConfig {
        env: vec![("OC_PLUGIN_TEST_SANDBOX".to_owned(), sandbox.display().to_string())],
        ..PluginHostConfig::default()
    };
    let host = PluginHost::detect().start_detached_with(config, &[entry]);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)));

    // The durable bus + the post-ingest bridge: the listener is the
    // "store" (applied log) and publishes through the tap AFTER the
    // durable commit — the wave-1 contract.
    let dir = tempfile::Builder::new()
        .prefix("oc-plugin-host-bus-")
        .tempdir_in("/tmp/opencode")
        .expect("temp dir");
    let bus = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(async {
            let db = oc_core::database::Database::open_file(dir.path().join("opencode.db"))
                .await
                .expect("db");
            oc_core::bus::Bus::new(
                db,
                oc_core::bus::BusOptions {
                    clock: Arc::new(oc_util::determinism::FixedClock::epoch()),
                    ids: Arc::new(oc_util::determinism::SequentialIdGen::new()),
                    persist: true,
                    ..oc_core::bus::BusOptions::deterministic()
                },
            )
        });
    let applied: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
    let tap = host.tap();
    let _listen_guard = {
        let applied = Arc::clone(&applied);
        let tap = tap.clone();
        bus.listen(Arc::new(move |event| {
            applied.lock().unwrap().push(event.id.as_str().to_string());
            tap.publish(TapEvent {
                id: event.id.as_str().to_string(),
                typ: event.type_.clone(),
                data: event.data.clone(),
                created: Some(event.created),
            });
            Ok(())
        }))
    };

    // The scripted sequence: six durable publishes in order.
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    rt.block_on(async {
        for n in 0..6 {
            bus.publish(
                renamed_def(),
                serde_json::json!({ "sessionID": "ses_g11", "title": format!("t{n}") }),
                oc_core::bus::PublishOptions::default(),
            )
            .await
            .expect("publish");
        }
    });

    // The G11-F1 window: a REPLAYING consumer (the reconnecting
    // client's Last-Event-ID resync) re-delivers everything after seq 3
    // through a naive bridge — the same ids the live listener already
    // published (the snapshot/live overlap).
    let replayed: Vec<String> = rt.block_on(async {
        use futures::StreamExt;
        let mut log = bus.log(oc_core::bus::LogInput {
            aggregate_id: "ses_g11".into(),
            after: Some(3),
            follow: false,
        });
        let mut ids = Vec::new();
        while let Some(item) = log.next().await {
            match item.expect("log item") {
                oc_core::bus::LogItem::Event(event) => ids.push(event.id.as_str().to_string()),
                oc_core::bus::LogItem::Synced(_) => break,
            }
        }
        ids
    });
    assert_eq!(replayed.len(), 2, "the replay covers the overlap window");
    for id in &replayed {
        assert!(
            applied.lock().unwrap().contains(id),
            "the replayed ids were already applied live (the G11-F1 shape)"
        );
        // The naive bridge publishes them again — the tap must dedup.
        tap.publish(TapEvent {
            id: id.clone(),
            typ: "session.renamed".into(),
            data: serde_json::json!({}),
            created: None,
        });
    }
    assert_eq!(tap.deduped_count(), 2, "the overlap was dropped exactly once each");

    // The plugin's observed stream matches the applied set 1:1, order
    // included — no duplicates, no reordering, no drops.
    let observed_path = sandbox.join("observed-ids.txt");
    let applied_log = applied.lock().unwrap().clone();
    assert!(common::wait_for(Duration::from_secs(10), || {
        common::read(&observed_path).lines().count() >= applied_log.len()
    }));
    let observed: Vec<String> = common::read(&observed_path).lines().map(str::to_string).collect();
    assert_eq!(observed, applied_log, "plugin's stream == the store's applied set, 1:1 in order");
    assert_eq!(
        observed.iter().collect::<std::collections::HashSet<_>>().len(),
        observed.len(),
        "no duplicate deliveries"
    );

    host.shutdown();
}
