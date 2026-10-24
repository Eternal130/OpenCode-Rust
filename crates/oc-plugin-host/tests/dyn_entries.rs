//! G-05 dynamic generation lifecycle: `PluginHost::add_entry` /
//! `remove_entry` — a source added mid-run boots its generation
//! (commands/routes/slot claims appear), removal tears it down
//! (registrations disappear), and a BROKEN add is contained (existing
//! generations unaffected). Bun-gated, sandboxed, never touches repo
//! fixtures (the `hot_reload` convention).

mod common;

use std::time::Duration;

/// The loaded-generation probe: one plugin's registrations keyed by
/// what it registers.
#[test]
fn dynamic_add_boots_generation_and_remove_tears_it_down() {
    let sandbox = common::sandbox("dyn");
    let hello = common::copy_fixture(&sandbox, "helloworld.ts");
    let claims = common::copy_fixture(&sandbox, "slotclaims.ts");
    let host = common::host_with(std::slice::from_ref(&hello), &sandbox);
    if !host.enabled() {
        eprintln!("bun unavailable; skipping");
        return;
    }
    assert!(host.wait_ready(Duration::from_secs(15)));

    let has = |name: &str| host.registrations().commands.iter().any(|command| command.name == name);
    let claims_live = || {
        let snapshot = host.registrations();
        snapshot.slot_claims.iter().any(|claim| claim.plugin_id == "slotclaims")
            && snapshot.routes.iter().any(|route| route.name == "board")
    };
    assert!(has("hw.hello"), "the boot entry loaded");

    // ADD mid-run: the slotclaims generation boots and its claims,
    // routes and registrations appear.
    host.add_entry(&claims).expect("add entry");
    assert!(
        common::wait_for(Duration::from_secs(10), claims_live),
        "the added generation's slot claims and routes registered"
    );

    // A BROKEN add is contained: badshape fails to load, both live
    // generations keep their registrations.
    let bad = common::copy_fixture(&sandbox, "badshape.ts");
    host.add_entry(&bad).expect("add call itself degrades to Ok (contained)");
    assert!(common::wait_for(Duration::from_secs(10), claims_live), "claims survive the bad add");
    assert!(has("hw.hello"), "the boot generation survives the bad add");
    let state_failed = |path: &std::path::Path| {
        host.registrations().plugins.iter().any(|status| {
            status.entry == path
                && matches!(status.state, oc_plugin_host::registrations::PluginState::Failed(_))
        })
    };
    assert!(state_failed(&bad), "the broken entry is marked failed, not silently dropped");

    // REMOVE: the slotclaims generation tears down and its claims
    // disappear; the boot generation is untouched.
    assert!(host.remove_entry(&claims).expect("remove entry"));
    assert!(
        common::wait_for(Duration::from_secs(10), || !claims_live()),
        "the removed generation's claims and routes dropped"
    );
    assert!(has("hw.hello"), "the boot generation survives the removal");

    // Idempotent remove: an unknown entry reports false.
    assert!(!host.remove_entry(&claims).expect("remove unknown"));

    host.shutdown();
}
