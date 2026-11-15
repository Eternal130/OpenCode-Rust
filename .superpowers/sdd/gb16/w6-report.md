# W6 — stale emit cache: root-cause + fix (G-429)

Branch `gb16-w5` (HEAD built on `7f0d8ac6`), lane continuation of the W5
follow-up. Result: **root cause found and fixed; evt_deliv re-landed;
`composer_tabs_walk` deterministic-green; full battery green.**

## Root-cause layering (probe chain, all logs in `.test-logs/oc-tui-gb16-w6-*`)

Experiment bed = the W5-R2 deterministic repro (evt_deliv in
`tabs_script`, reverted at W5). Re-applied as the first step; walk red
3/3 at "← never returned to Shell" (~8.1 s), same as W5.

1. **Emitter exonerated** (`opentui_emit.rs` probe: composed frame vs
   `current_render_buffer` vs `next_render_buffer` at the row carrying
   "return"): the COMPOSED FRAME ITSELF is corrupt —
   `frame: "tabsut return …"` while `cur: "tabs ←/→"`, `next == frame`.
   `OpentuiEmitter`'s diff (and opentui-core's per-cell word diff) is
   faithful: it correctly emitted only the cells that differ. The W5
   suspicion ① (emit cache) and ② (OptimizedBuffer word diff) are both
   cleared.
2. **Composite seam exonerated** (`chrome_solid.rs` probe: the solid
   tree's raster row vs the hand-painted base row at the seam): the
   solid tree's own `current_render_buffer` row already reads
   `tabsut return …`; the base row under the panel is blank. The
   corruption is painted INTO the shared-tree raster (probe2 log: row
   27 across phases, incl. the mid-transition `"tabs inactive ctrl+a"`
   mixing frame).
3. **Inputs exonerated** (`picker_solid.rs` probe: the `PanelInputs`
   signal payload each mount): always correct per tab (Shell:
   `u@9="output"@x5`, `u@9#17=" return"@x11`, …; Terminals:
   `u@9="tabs"@x5`, `u@9#17=" ←/→"@x9`). The walk
   (`picker_panel_paint`) is stateless and correct — confirmed.
4. **Duplicate same-key node in the solid render tree** (tree probe:
   count `u@9` nodes per composite): from the Shell→Terminals switch
   on, EVERY frame has n=2 `u@9` text nodes at x=5 — one carrying the
   fresh content ("output"), one a zombie stuck at "tabs" (from the
   Terminals phase). The zombie paints AFTER the fresh node (later
   child = later paint), overwriting its first 4 cells →
   `tabs` + `ut return` = `tabsut return`. Exactly the observed
   screen and the exact 4-cell skip the byte capture showed.
5. **Where the duplicate is born** (opentui-solid probes via a
   temporary `[patch]` to `/opt/opentui-rust`, since removed):
   `reconcile_children`'s keyed loop. On Shell→Terminals the keyed
   lists churn wholesale (body-row tag keys change
   `t:picker:entry:sh_1@3`→`t:picker:entry:@3`, hints shrink
   `u@9…#21`→`u@9,#17`, and an earlier insert pass had left the
   working list's tail order scrambled
   `[…,u@9,#18,#19,#20,#21,#17]`). The map+sequence heuristic then
   takes the REPLACE arm for `u@9`'s slot (mount a new element there,
   unmount the old by live-id), and the tail of the same pass
   re-INSERTS `u@9` fresh (mount probe: `u@9` mounted at index 17 AND
   index 18 in the same transition). The replace/unmount bookkeeping
   desyncs the shadow working list from the live render subtree, and a
   duplicate `u@9` renderable survives in the render tree forever
   (subsequent content patches reach only the shadow-first node; the
   zombie is never updated nor removed).

**Root cause layer: opentui-solid (external git dependency, rev
`34a5ac4`) — keyed-For `reconcile_children` same-key duplicate mount
on replace/insert churn.** Per the lane boundary (external crate ⇒ no
in-repo fix), the fix is the oc-tui-side guard below + escalation.

## Fix (oc-tui-side legitimate guard)

`picker_solid.rs`: the picker content subtree's rebuild key
(`colors_key`) now carries the ACTIVE TAB discriminant
(`picker_tab_discriminant`) in addition to width + baked colors. A tab
switch — the exact churn shape that trips the upstream reconcile bug —
now REMOUNTS the panel content wholesale through the existing
`content_key` seam (built for identity-semantic rebuilds), instead of
patching keyed units in place. Selection moves / filter flips / entry
arrivals keep the in-place patch path (long-stable). Invalidation
semantics documented on `colors_key` with the G-429 cross-reference.

## evt_deliv re-land (W5-R2's 5-line patch, now permanent)

`pty_composer_tabs.rs::tabs_script` gains `evt_deliv`
(`session.inbox.delivered` for `msg_user_1`, delay 200) after
`evt_e`: the scripted session's pending row clears, the session truly
goes idle, the spinner/pump rests — the walk's end-state contract is
complete, and the walk now PINS the sparse-repaint correctness the
busy pump used to paper over. The 6 s wait windows stay (CI slow-runner
margin, comment already in place).

## Verification (all via `tools/dev-gate.sh`, full logs teed)

* Repro green 3/3: `composer_tabs_walk` (evt_deliv variant) —
  `1 passed` × 3 consecutive runs, ~3.0 s each (was deterministic
  8.1 s red).
* Full suite: `cargo nextest run -p oc-tui --locked --no-fail-fast` —
  `Summary [229.5s] 1703 tests run: 1703 passed (1 flaky), 7 skipped`;
  the one flaky (`pty_composer_cycle_type_edit_submit_reply_stats_solid_path`,
  "the usage arm rendered on the completed turn", 25 s under full-suite
  load) passed the same-round `--exact` retry (1.9 s) — usage/stats
  timing flake, not picker-related, not previously whitelisted; noted
  as a concern below. Gold-frame/tree oracles: no drift (the fix
  changes WHEN the picker content remounts, never frame content;
  picker goldens mount fresh per call by construction).
* PTY profile: `--profile pty` — `Summary [878.2s] 1703 tests run:
  1703 passed, 7 skipped`.
* Channel 8 full: `TUI_DIFF_BUN=… python3 tools/diff/tui_fullscreen.py`
  — **gate exit 0**; committed distribution unchanged (regenerated
  `docs/audit/diff/TUI-FULLSCREEN.md` is byte-identical to the
  committed file — git reports no diff; divergences are the same
  adjudicated CAUSES set A2/B2/G-424b/G-426; grid-level comparison
  unaffected by the emission path, as expected).
* `cargo clippy --workspace --all-targets --locked -- -D warnings`:
  clean. `cargo fmt --all -- --check`: clean (after one `cargo fmt
  --all`). `bash tools/guards/run.sh`: `guards: all green`.

## Registration

* `docs/GAPS-AND-PITFALLS.md`: new **G-429** entry (symptom / byte
  evidence / root-cause layer / fix / why the busy pump masked it /
  upstream escalation note) + flake-whitelist annotation:
  `composer_tabs_walk` **removed as fixed** (reappearance = regression).
* `TUI-FULLSCREEN.md` not regenerated (distribution identical).

## Concerns

1. **Upstream escalation required**: the opentui-solid
   `reconcile_children` duplicate-mount defect is GENERAL (any
   keyed-For consumer with replace/insert churn can hit it). The
   picker guard is a workaround at one consumer; once upstream fixes
   the reconcile (live-list/render-subtree desync in the
   replace/insert arms), the tab arm of the rebuild key can be dropped
   to restore pure in-place patching. Evidence chain for the upstream
   ticket: this report + `.test-logs/oc-tui-gb16-w6-probe*` (probe
   code itself was temporary and fully removed; external checkout
   `/opt/opentui-rust` reverted clean, `git status` empty).
2. `pty_composer_cycle_type_edit_submit_reply_stats_solid_path` showed
   a load-flake (TRY1 fail, exact-retry pass) in the full-suite round —
   outside this lane's scope, not whitelisted; flagging for the
   orchestrator's flake ledger.
