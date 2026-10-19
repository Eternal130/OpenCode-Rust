//! The plugin UI-slot router (PLUG-TUI-SLOT) — a faithful port of the
//! ref's pure slot-tree resolution, `packages/tui/src/plugin/
//! structure.ts` (line-for-line policy: replacement takeover,
//! hierarchy-beats-timeline, last-enabled-wins, missing-target
//! degradation). No rendering, no I/O — every policy rule is a data
//! transform, exactly like the ref's module header demands.
//!
//! The TUI's mounted slot paths (the ref mounts `<Slot path=…>`
//! components; the native surfaces here are the app root and the
//! sidebar's plugin content column):
//!
//! * `app` — the app root (feature plugins claim it to install
//!   keymap layers at mount, ref `feature-plugins/system/stats.tsx:
//!   185-203`);
//! * `sidebar.content` — the docked sidebar's plugin column (the
//!   `SidebarMcp`/`SidebarContext` mount point, ref
//!   `feature-plugins/sidebar/mcp.tsx:89-96`).

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use oc_plugin_host::registrations::PluginSlotClaim;

/// The ref's placement kinds (structure.ts:7).
pub const PLACEMENT_KINDS: [&str; 5] = ["prepend", "append", "before", "after", "replace"];

/// The native slot paths the TUI mounts (see the module docs).
pub const MOUNTED_SLOT_PATHS: [&str; 2] = ["app", "sidebar.content"];

/// One resolved claim (the snapshot's [`PluginSlotClaim`] mirrored at
/// the router's edge — `render` stays in the guest; the TUI
/// materializes content through the host's bounded
/// `render_slot_claim` call).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotClaim {
    /// Counter-suffixed claim key (`slot#0`).
    pub key: String,
    /// Owning plugin id.
    pub plugin: String,
    /// `prepend` | `append` | `before` | `after` | `replace`.
    pub kind: &'static str,
    /// The targeted slot path.
    pub target: String,
}

/// Everything one mounted slot renders besides its own children
/// (structure.ts:24-31): siblings around the boundary, contributions
/// inside it, and at most one takeover.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Slotted {
    pub before: Vec<SlotClaim>,
    pub prepend: Vec<SlotClaim>,
    pub append: Vec<SlotClaim>,
    pub after: Vec<SlotClaim>,
    /// The winning replacement (last-enabled per target).
    pub replace: Option<SlotClaim>,
}

/// A claim suppressed by a conflict or boundary takeover
/// (structure.ts:33-39). `by` is absent when a replacement's target
/// no longer exists (missing replacements never degrade).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suppressed {
    pub claim: SlotClaim,
    pub by: Option<SlotClaim>,
}

/// A claim whose target is gone, degraded to appending on the
/// nearest surviving ancestor (structure.ts:41-45).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Degraded {
    pub claim: SlotClaim,
    /// The surviving ancestor path the claim appended to.
    pub to: String,
}

/// The resolution output: per-path placement buckets plus the
/// diagnostics.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SlotResolution {
    pub slotted: BTreeMap<String, Slotted>,
    pub suppressed: Vec<Suppressed>,
    pub degraded: Vec<Degraded>,
}

/// The tree's one containment rule (structure.ts:47-49): a path
/// contains every path it prefixes.
fn contains(ancestor: &str, path: &str) -> bool {
    path.starts_with(&format!("{ancestor}."))
}

fn depth(path: &str) -> usize {
    path.split('.').count()
}

fn surviving_ancestor(path: &str, paths: &BTreeSet<String>) -> Option<String> {
    let mut index = path.rfind('.');
    while let Some(at) = index {
        let ancestor = &path[..at];
        if paths.contains(ancestor) {
            return Some(ancestor.to_owned());
        }
        index = if at == 0 { None } else { path[..at].rfind('.') };
    }
    None
}

/// Resolve the slot tree (structure.ts:60-160, verbatim policy):///
/// * pass 1 — replacement boundaries: per target the last-enabled
///   claim wins; an accepted boundary swallows every replacement
///   strictly inside it regardless of enable order (hierarchy beats
///   timeline);
/// * pass 2 — additive claims in enable order: a claim inside a
///   replaced boundary is suppressed (`prepend`/`append` targeting
///   the boundary itself are part of its contents; `before`/`after`
///   on it stay outside); a claim whose target is gone degrades to
///   appending on the nearest surviving ancestor (replacements never
///   degrade).
#[must_use]
pub fn resolve_slots(paths: &BTreeSet<String>, claims: &[SlotClaim]) -> SlotResolution {
    let mut suppressed: Vec<Suppressed> = Vec::new();
    let mut degraded: Vec<Degraded> = Vec::new();

    // Pass 1 — replacement boundaries.
    let mut winners: BTreeMap<&str, &SlotClaim> = BTreeMap::new();
    for claim in claims {
        if claim.kind != "replace" {
            continue;
        }
        if !paths.contains(&claim.target) {
            suppressed.push(Suppressed { claim: claim.clone(), by: None });
            continue;
        }
        match winners.get(claim.target.as_str()) {
            Some(prior) => {
                suppressed.push(Suppressed { claim: (*prior).clone(), by: Some(claim.clone()) });
                winners.insert(claim.target.as_str(), claim);
            }
            None => {
                winners.insert(claim.target.as_str(), claim);
            }
        }
    }
    let mut boundaries: BTreeMap<String, SlotClaim> = BTreeMap::new();
    let containing = |boundaries: &BTreeMap<String, SlotClaim>, path: &str| {
        boundaries
            .iter()
            .find(|(boundary, _)| contains(boundary, path))
            .map(|(_, winner)| winner.clone())
    };
    // Shallow boundaries first, so a nested replacement meets its
    // container.
    let mut ordered: Vec<(&str, &SlotClaim)> = winners.iter().map(|(k, v)| (*k, *v)).collect();
    ordered.sort_by_key(|(path, _)| depth(path));
    for (path, winner) in ordered {
        if let Some(outer) = containing(&boundaries, path) {
            suppressed.push(Suppressed { claim: winner.clone(), by: Some(outer) });
            continue;
        }
        boundaries.insert((*path).to_owned(), winner.clone());
    }

    // Pass 2 — additive claims, in enable order.
    let mut buckets: BTreeMap<String, Slotted> = BTreeMap::new();
    for claim in claims {
        if claim.kind == "replace" {
            continue;
        }
        let target = claim.target.as_str();
        // Inside placements targeting a replaced boundary are part of
        // its contents; sibling placements on the boundary itself
        // stay outside it.
        let inside = if claim.kind == "prepend" || claim.kind == "append" {
            boundaries.get(target).cloned()
        } else {
            None
        };
        let outer = inside.or_else(|| containing(&boundaries, target));
        if let Some(outer) = outer {
            suppressed.push(Suppressed { claim: claim.clone(), by: Some(outer) });
            continue;
        }
        if paths.contains(target) {
            let bucket = buckets.entry(target.to_owned()).or_default();
            bucket_for(bucket, claim.kind).push(claim.clone());
            continue;
        }
        if let Some(ancestor) = surviving_ancestor(target, paths) {
            // The ancestor is never a replaced boundary (containing
            // caught that); appending is load-bearing (structure.ts:
            // 137-140 — a parent slot renders append after its
            // children, so the transient degradation never
            // instantiates anything).
            degraded.push(Degraded { claim: claim.clone(), to: ancestor.clone() });
            buckets.entry(ancestor).or_default().append.push(claim.clone());
            continue;
        }
        suppressed.push(Suppressed { claim: claim.clone(), by: None });
    }

    for (path, winner) in boundaries {
        let bucket = buckets.entry(path).or_default();
        bucket.replace = Some(winner);
    }
    SlotResolution { slotted: buckets, suppressed, degraded }
}

fn bucket_for<'a>(bucket: &'a mut Slotted, kind: &str) -> &'a mut Vec<SlotClaim> {
    match kind {
        "before" => &mut bucket.before,
        "prepend" => &mut bucket.prepend,
        "append" => &mut bucket.append,
        _ => &mut bucket.after,
    }
}

/// Project the snapshot's registration-order claims into the router's
/// claim shape (placement validated at the host boundary; unknown
/// kinds are filtered here — contained, like every plugin surface).
#[must_use]
pub fn project_claims(claims: &[PluginSlotClaim]) -> Vec<SlotClaim> {
    claims
        .iter()
        .filter_map(|claim| {
            let kind = PLACEMENT_KINDS.iter().find(|kind| **kind == claim.kind)?;
            Some(SlotClaim {
                key: claim.key.clone(),
                plugin: claim.plugin_id.clone(),
                kind,
                target: claim.target.clone(),
            })
        })
        .collect()
}

/// The mounted path set (the native surfaces above).
#[must_use]
pub fn mounted_paths() -> BTreeSet<String> {
    MOUNTED_SLOT_PATHS.iter().map(|path| (*path).to_owned()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claim(key: &str, plugin: &str, kind: &str, target: &str) -> SlotClaim {
        SlotClaim {
            key: key.to_owned(),
            plugin: plugin.to_owned(),
            kind: PLACEMENT_KINDS.iter().find(|k| **k == kind).copied().unwrap(),
            target: target.to_owned(),
        }
    }

    fn paths(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|path| (*path).to_owned()).collect()
    }

    /// Additive claims land in their target's buckets in enable
    /// order (structure.ts:119-123).
    #[test]
    fn additive_claims_bucket_in_enable_order() {
        let resolution = resolve_slots(
            &paths(&["sidebar.content"]),
            &[
                claim("slot#0", "omos", "append", "sidebar.content"),
                claim("slot#1", "stats", "append", "sidebar.content"),
                claim("slot#2", "omos", "prepend", "sidebar.content"),
            ],
        );
        let bucket = &resolution.slotted["sidebar.content"];
        assert_eq!(bucket.append.len(), 2);
        assert_eq!(bucket.append[0].plugin, "omos");
        assert_eq!(bucket.append[1].plugin, "stats");
        assert_eq!(bucket.prepend.len(), 1);
        assert!(resolution.suppressed.is_empty(), "no suppression: {:?}", resolution.suppressed);
    }

    /// Last-enabled replacement wins per target; the prior winner is
    /// suppressed `by` the newcomer (structure.ts:70-78).
    #[test]
    fn replacement_last_enabled_wins() {
        let resolution = resolve_slots(
            &paths(&["app"]),
            &[claim("slot#0", "a", "replace", "app"), claim("slot#1", "b", "replace", "app")],
        );
        let bucket = &resolution.slotted["app"];
        assert_eq!(bucket.replace.as_ref().expect("the winner").plugin, "b");
        assert_eq!(resolution.suppressed.len(), 1);
        assert_eq!(resolution.suppressed[0].claim.plugin, "a");
        assert_eq!(resolution.suppressed[0].by.as_ref().expect("by the winner").plugin, "b");
    }

    /// Hierarchy beats timeline: a replacement strictly inside an
    /// accepted boundary is swallowed regardless of enable order
    /// (structure.ts:80-97); `before`/`after` ON the boundary stay
    /// outside it while `prepend`/`append` on it are part of its
    /// contents (structure.ts:125-133).
    #[test]
    fn hierarchy_beats_timeline() {
        let resolution = resolve_slots(
            &paths(&["app", "app.body"]),
            &[
                claim("slot#0", "inner", "replace", "app.body"),
                claim("slot#1", "outer", "replace", "app"),
                // after on the boundary itself stays OUTSIDE it.
                claim("slot#2", "sibling", "after", "app"),
                // prepend INSIDE the replaced boundary is suppressed.
                claim("slot#3", "cousin", "prepend", "app.body"),
                // append ON the boundary is part of its contents.
                claim("slot#4", "inside", "append", "app"),
            ],
        );
        let bucket = &resolution.slotted["app"];
        assert_eq!(bucket.replace.as_ref().expect("the outer winner").plugin, "outer");
        assert_eq!(bucket.after[0].plugin, "sibling", "the boundary's own after survives");
        assert!(bucket.append.is_empty(), "append on the boundary is inside it");
        let inner =
            resolution.suppressed.iter().map(|s| s.claim.plugin.clone()).collect::<Vec<_>>();
        assert!(inner.contains(&"inner".to_owned()), "the nested replacement is swallowed");
        assert!(inner.contains(&"cousin".to_owned()), "the inside prepend is suppressed");
        assert!(inner.contains(&"inside".to_owned()), "the boundary append is suppressed");
    }

    /// A claim whose target is gone degrades to appending on the
    /// nearest surviving ancestor; a missing replacement is
    /// suppressed without a winner (structure.ts:124-150).
    #[test]
    fn missing_targets_degrade_or_suppress() {
        let resolution = resolve_slots(
            &paths(&["sidebar.content"]),
            &[
                claim("slot#0", "omos", "append", "sidebar.content.gone"),
                claim("slot#1", "omos", "replace", "sidebar.content.gone"),
            ],
        );
        assert_eq!(resolution.degraded.len(), 1);
        assert_eq!(resolution.degraded[0].to, "sidebar.content");
        assert_eq!(resolution.slotted["sidebar.content"].append[0].plugin, "omos");
        assert_eq!(resolution.suppressed.len(), 1, "the missing replacement never degrades");
        assert!(resolution.suppressed[0].by.is_none());
    }

    /// The snapshot projection filters unknown kinds (the host
    /// validates; the router stays total).
    #[test]
    fn projection_filters_unknown_kinds() {
        use oc_plugin_host::registrations::PluginSlotClaim as Raw;
        let claims = project_claims(&[
            Raw {
                plugin_id: "omos".into(),
                key: "slot#0".into(),
                kind: "append".into(),
                target: "sidebar.content".into(),
            },
            Raw {
                plugin_id: "omos".into(),
                key: "slot#1".into(),
                kind: "sideways".into(),
                target: "app".into(),
            },
        ]);
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].kind, "append");
    }
}
