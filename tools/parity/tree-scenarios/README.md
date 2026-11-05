# tree-scenarios — the oc-tui production-tree captures (Lane TB)

One JSON per representative application state: the SETTLED mounted
shared tree (the `chrome_solid` hub walk — `oc_tui::components::
tree_capture`) serialized as an **opentui-diff tree scenario** (the
v1 contract in opentui-rust `tools/diff/README.md` "Tree scenarios").
`oracle/` holds the frozen TS-oracle frames (the REAL `@opentui
0.5.14` driven by bun — same capture shape, fingerprinted).

## Layout

* `tree-*.json` — the captured scenario (committed; `_meta.source`
  names the state, `_meta.skipped_kinds` the ink outside the v1
  contract — composer textarea/input rows — counted, both engines
  render the captured subset identically).
* `oracle/tree-*.json` — the frozen oracle frame per scenario. The
  standing nextest gate (`oc-tui::tree_scenarios::
  dual_engine_matches_frozen_oracle`) replays each scenario through
  opentui's own tree rebuilder and compares cell-for-cell — no bun
  needed. All 7 scenarios match the frozen oracle IDENTICALLY (the
  Lane TB `tree-diff-viewer` port mismatch was healed by the
  opentui-core bump to 2fbe00e — oracle re-frozen, pin dropped).

## Adding a scenario

1. Add a `StateSpec` to `crates/oc-tui/tests/tree_scenarios.rs` (a
   `Mount::App` drive closure over the store, or a twin mount).
2. `tools/parity/tree-scenario-capture.sh` (regenerates all JSON).
3. `TREE_ORACLE_FREEZE=1 tools/parity/tree-oracle-freeze.sh` (re-freezes
   the TS-oracle frames — needs bun + the sandbox install).
4. Commit scenario + oracle together; both gates must stay green.

## Regeneration discipline

Both scripts are double-gated (explicit invocation AND an env var) —
the PARITY_UPDATE stop-and-escalate rule. Never re-freeze an oracle
to make a red gate green; a red gate is a finding.
