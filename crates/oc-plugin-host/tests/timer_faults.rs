//! SA-F16 (SCAN-AB) — plugin-host hardening on unreadable dirs: a
//! plugin's PERIODIC filesystem probing that hits permission-denied
//! paths (the scan's `uncaughtException: EACCES /lost+found` /
//! `/tmp/tmux-0` during health-check-driven probing) must degrade to
//! a recorded fault — never a process-level uncaught throw — and the
//! guest must stay healthy (commands dispatch, the probing timer keeps
//! running contained).
//!
//! The host.js timer shields wrap every scheduled guest callback
//! (sync throw + async rejection) into a `plugin.fault` notification
//! the supervisor records; the process-level handlers remain the last
//! resort.

mod common;

use std::time::Duration;

#[test]
fn scheduled_guest_faults_degrade_to_recorded_health_results() {
    let sandbox = common::sandbox("timercrasher");
    let entry = common::copy_fixture(&sandbox, "timercrasher.ts");
    let host = common::host_with(&[entry], &sandbox);
    assert!(host.wait_ready(Duration::from_secs(20)), "host ready");

    // The probe timer fired and faulted — recorded as health results
    // (both the sync EACCES throw and the async rejection arm).
    assert!(
        common::wait_for(Duration::from_secs(10), || {
            let faults = host.plugin_faults();
            faults.iter().any(|fault| fault.contains("EACCES"))
                && faults.iter().any(|fault| fault.contains("async probe failed"))
        }),
        "the shielded faults landed: {:?}",
        host.plugin_faults()
    );
    // The faulting callback STOPPED at the throw (never.txt absent) —
    // the shield swallowed it, the write after it never ran.
    assert!(!sandbox.join("never.txt").exists(), "the faulting callback stopped at the throw");
    // The guest is HEALTHY through repeated faults: dispatch answers.
    let outcome = host.execute_command("timercrasher.ping", None, Duration::from_secs(10));
    let alive = outcome.as_ref().is_ok_and(|value| value.to_string().contains("alive-after-"));
    assert!(alive, "dispatch stays live through repeated faults: {outcome:?}");
    host.shutdown();
}
