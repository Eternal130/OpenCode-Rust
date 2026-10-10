//! The deadlock/reentrancy storm (deliverable 1's fuzz-ish stress):
//! nested guest→host sync calls from inside `load` (30 registrations
//! under one outstanding reply), from inside every event handler, and
//! from inside command executions — all against concurrent tap
//! publishes from multiple threads plus a hot-reload scan. No
//! deadlock, bounded time, host responsive at the end.

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use oc_plugin_host::TapEvent;

#[test]
fn nested_reentrant_call_storm_no_deadlock() {
    let sandbox = common::sandbox("storm");
    let storm = common::copy_fixture(&sandbox, "storm.ts");
    let host = Arc::new(common::host_with(std::slice::from_ref(&storm), &sandbox));
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    let started = Instant::now();
    assert!(
        host.wait_ready(Duration::from_secs(15)),
        "setup's 30 nested hostRequests resolved under the load reply: {:?}",
        host.start_error()
    );
    let snapshot = host.registrations();
    assert_eq!(snapshot.commands.len(), 30, "all nested registrations landed");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "registration pass bounded (took {:?})",
        started.elapsed()
    );

    // The storm: two publisher threads × 100 events, concurrent
    // command executions (each nests 5 sync pings guest-side), and a
    // concurrent digest scan — all live at once.
    let tap = host.tap();
    let publishers: Vec<_> = (0..2)
        .map(|worker| {
            let tap = tap.clone();
            std::thread::spawn(move || {
                for n in 0..100 {
                    let payload = serde_json::json!({ "worker": worker, "n": n });
                    tap.publish(TapEvent {
                        id: format!("evt_storm_{worker}_{n}"),
                        typ: if n % 2 == 0 { "storm.tick" } else { "storm.echo" }.to_owned(),
                        data: payload,
                        created: Some(1_700_000_000_000 + u64::try_from(n).unwrap_or(0)),
                    });
                }
            })
        })
        .collect();
    let executors: Vec<_> = (0..2)
        .map(|_| {
            let host = Arc::clone(&host);
            std::thread::spawn(move || {
                for i in 0..5u32 {
                    let name = format!("storm.{i}");
                    let outcome = host.execute_command(&name, None, Duration::from_secs(10));
                    assert!(
                        outcome.is_ok(),
                        "execute {name} during the storm: {:?}",
                        outcome.unwrap_err()
                    );
                }
            })
        })
        .collect();
    let reloader = {
        let host = Arc::clone(&host);
        std::thread::spawn(move || {
            let _ = host.reload_now();
        })
    };

    for publisher in publishers {
        publisher.join().expect("publisher");
    }
    for executor in executors {
        executor.join().expect("executor did not deadlock");
    }
    reloader.join().expect("reloader");

    // The pump drains sequentially — wait for it to catch up with all
    // 200 events (each carries a nested sync ping) before judging.
    let drained = common::wait_for(Duration::from_secs(25), || {
        let (dispatched, dropped) = host.dispatch_counters();
        dispatched + dropped == 200
    });
    let elapsed = started.elapsed();
    assert!(elapsed < Duration::from_secs(30), "the whole storm stayed bounded (took {elapsed:?})");
    assert!(drained, "every event outcome decided");

    // Every event dispatched exactly once, none dropped.
    let (dispatched, dropped) = host.dispatch_counters();
    assert_eq!(dispatched, 200, "all storm events delivered (dropped {dropped})");
    assert_eq!(dropped, 0, "no dispatch dropped under the storm");

    // The host is still responsive afterwards.
    let after = host.execute_command("storm.29", None, Duration::from_secs(5));
    assert!(after.is_ok(), "host responsive post-storm");
    host.shutdown();
}
