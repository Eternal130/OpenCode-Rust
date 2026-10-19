# SCAN-AB — Axes A (Visual/Rendering) + B (Interaction/Input/Environment)

**Lane:** SCAN-AB (first execution of docs/scan-spec.md **v9 LOCKED**).
**Worktree:** `.worktrees/lane-scanab` (branch `lane/scanab`, base `a73e183` = main).
**Date:** 2026-10-17/18. **Product code READ-ONLY** — artifacts: this file,
`docs/scanab.log`, scaffolds `crates/oc-tui/tests/spec_scanab_a1.rs` +
`tools/parity/spec-scanab/a1_cell_diff.py`, captures under `/tmp/opencode/scanab/`.

**Instruments used (spec I-layer):** I1 real payloads via our own FakeLlm
server chain (in-process `run_with_size`, real PTY); I2/I2b ref+plugin ground
truth = ref **2.0.25 binary** (`/root/.opencode/bin/opencode`) + omo-slim, the
standing `rescan-tui.rs` captures (`/tmp/opencode/rescan-{live,time}`, real-run,
I9: real; ref rig env recorded 2026-10-17 — SSH-state unrecorded there, see
SA-F1 disposition note); I3 harness clock (per-burst snapshots); I4 token-exact
style digests (truecolor RGB / xterm-256 indexed, decoded + palette-expanded);
I8 canaries RUN before first evidence per axis; I6 attribution per finding.

**Severity classes:** S0 = daily user-visible breakage; S1 = user-visible
degradation / reachability; S2 = fidelity/edge/note.

---

## A1 — Theme tokens × element × STATE matrix

**I8 canary (tier rule): RUN + PASS before first A1 evidence.**
`a1_canary_leg_default` + `a1_canary_leg_swap_verdict` (process-isolated legs;
full-stack token-cell swap: complete-builtin theme doc with
`base.text.base`↔`base.text.muted` hex swapped, planted via
`OPENCODE_CONFIG_DIR/themes/` + selected via `OPENCODE_CLI_CONFIG_CONTENT`).
Verdict: `changed=244 base->muted=93 muted->base=110..149 foreign=0
derived-neutral=2..3` (derived = increase/decrease on the neutral axis,
dispositioned: #282828→#434343, #808080→#ffffff). PASS = the cell-capture
method is token-exact-sensitive. Re-run PASS after the TERM pin (instrument
change). Canary artifacts: `/tmp/opencode/scanab/a1/canary-*.{cells,styles.txt,txt}`.

**[G] coverage executed (cells with evidence):** spinner family (tab-strip
busy glyph (1,0) + transcript Thinking-row glyph, states base/busy/anim-phase —
8-phase series @120 ms), tab strip (active/inactive/`+` affordance; hover
injectors), sidebar MCP rows + headers (selected/connect states), dialog rows
(unselected vs ArrowDown-selected), home surface (wordmark/getting-started
card/composer), footer left/right. **Numeric edge sweep:** footer context/usage
cells ride the rescan-live ref pair (ours-fixture lacks context data — SA-F2).
**States not cell-captured this lane:** error-row (ours golden
`assistant_footer_with_error_*` exists — ours-side only), disabled, expanded,
interrupted — CARRIED (see ledger).

**Evidence pair (ours fresh @ lane-scanab vs ref 2.0.25 real captures):**

| cell | verdict | ours | ref 2.0.25 | notes |
|---|---|---|---|---|
| tab-strip busy glyph (1,0) | **token PARITY / emit-kind DIVERGE** | `⠙` #fab283 (truecolor) | `⠙` idx216→#ffaf87 | see SA-F1 |
| transcript Thinking glyph | ours animates (⠇⠏⠙⠹ series) | ours fixture-only this lane | ref series in rescan-time | rescan-F3 class verified fixed |
| sidebar 'MCP'[0..2] | RGB-MATCH/emit-diff | #eeeeee+B | idx255→#eeeeee+B | SA-F1 class |
| tab strip first cell | RGB-MATCH/emit-diff | #eeeeee+B | idx255→#eeeeee+B | SA-F1 class |
| sidebar 'Context'/'tokens'/'% used' | anchor-miss | fixture lacks context stats | ref renders them | SA-F2 (fixture-happy risk, I6-flagged) |

Analysis tool: `tools/parity/spec-scanab/a1_cell_diff.py` (digest decode +
xterm-256 expansion + anchored diff). Table artifact:
`/tmp/opencode/scanab/a1/a1-diff-table.md`.

### SA-F1 (S1) — color-mode probe divergence under SSH_*: ours emits truecolor, ref 2.0.25 emits indexed-256
- **Evidence:** ours `ours-busy-phase-02.styles.txt` (1,0) fg kind=rgb
  #fab283 vs ref `rescan-time/temporal/captures/ref-busy-00.styles.txt` (1,0)
  fg kind=idx 216 (#ffaf87) — same TERM=xterm-256color class, both real PTY
  runs. Token itself is IDENTICAL both trees (`hue.interactive.200`=#fab283 —
  `crates/oc-theme/assets/v2/opencode.json:329` =
  `/opt/opencode-ref/packages/tui/src/theme/assets/v2/opencode.json:329`).
- **Isolation (control leg, PASS):** `a1_ours_no_ssh_quantize_leg` — with
  SSH_{CONNECTION,CLIENT,TTY} removed, TERM=xterm-256color, ours quantizes
  fully (`indexed=5760 rgb=0`) and our `quantize()` maps #fab283→216 = ref's
  observed index (nearest-cube math verified). So quantizer math is at parity;
  the divergence is the **probe rule**: our ported table
  (`crates/oc-tui/src/render/color_mode.rs:69` `detect`: ssh⇒TrueColor) fires
  under SSH_*; the ref 2.0.25 binary quantized in a rig whose SSH-state is
  unrecorded.
- **Residual risk:** on ssh into 256-color-only terminals our colors are
  approximated by the terminal instead of deterministically pre-quantized
  (observed palette error (5,3,4) for the busy glyph); hue shifts possible on
  stricter terminals. Rides **B6** color-count-fallback cells (16-color leg)
  for the full disposition; a fresh ref leg with env recorded is queued in the
  A5 run.
- **I6 attribution:** inventory-grade evidence (live cells both sides);
  initial capture was env-confounded (gate env SSH_*), caught and isolated by
  the control leg — NOT fixture-happy, NOT adjudication-masked (per-cell, not
  umbrella).

### SA-F2 (S2) — sidebar Context/tokens/%-used cells unevaluable under FakeLlm (fixture gap)
- Ours-side matrix captures cannot render the sidebar context block (FakeLlm
  serves no context/token/usage stats); ref rescan-live renders it from the
  ScenarioHost chain. Disposition: ours-side cells CARRIED to a
  ScenarioHost-class fixture (or live server); overlaps rescan F4 (usage
  percent) which was evidenced REAL on the rescan rig — F4's fix state on main
  is NOT re-adjudicated here (fixture-bound). **I6: fixture-happy risk
  registered** — no parity claim made from missing cells.

### SA-F13 (S1) — second TUI instance in one process: teardown hang + dead exit chord
- Two `run_with_size` boots in one test process: leg-2 app renders home but
  ignores the exit chord (`\x03` + leader `q`), emits teardown bytes only
  sometimes, and the run-loop future does not return after `?1049l` (postmortem
  raw `timeout-a1can-b.raw`). Single-instance flows are clean (probes
  `a1_probe_config_exit_chord`, `a1_probe_theme_exit_chord` PASS in ~1.4 s).
- User-relevant shape: not directly reachable via the product CLI (one TUI per
  process), but it blocks legitimate embedders/tests; also E9-relevant
  (teardown residue class). Carried to B1/E9 for product disposition.

### SA-F14 (S2) — with a user config FILE present, the first exit chord is sometimes ignored
- Observed during canary bring-up with `cli.json` planted (pre-TEARDOWN-HANG
  runs); double-chord recovers. Probes with env-overlay config (`{}` and
  theme-selection) PASS — so the class is specifically config-FILE-present
  (file watcher?). Needs a dedicated repro (B1 keybind × config-variant cell
  + E9 teardown) — registered, carried.

### A1 notes (non-findings, recorded)
- Partial-override theme documents are INVALID in our resolver
  (`theme.base.text.action: missing required key` — swap theme had to derive
  from the complete builtin doc). Whether ref accepts partial overrides is an
  **A7** chain cell (theme discovery/apply) — noted for the A7 leg.
- `OPENCODE_CONFIG` env var is NOT in our TUI config chain (plugin-host only);
  TUI config = `roots.config/cli.json` + `OPENCODE_CLI_CONFIG_CONTENT`
  overlay. Matches the C4/B7 families; no parity claim made here.
- Composer placeholder suggestion rotates per boot (content-drift class,
  21–38 cells) — a real content-rotation behavior; A2/A3 own its geometry;
  noted so future diffs don't mis-adjudicate it as color drift.

---

## A4 — Temporal at vt-clock

**I8 canary status:** A4's method shares the A1 cell-capture instrument (canary
PASS covers the style-digest evidence); the phase leg's falsifiability = the
marquee cadence probe (a wrong period is directly measurable).

**Ours leg (vt-clock harness, fresh @ lane-scanab):**
- **Spinner family PASS (rescan-F3 fix verified on main):** tab-strip busy
  glyph (1,0) + transcript Thinking-row glyph both animate;
  `vt_temporal_spinner_animates_100ms_frames` PASS standalone — 33 char
  changes, median gap 108 ms, full 10-glyph braille cycle. Busy-phase series
  (8 × 120 ms) shows glyph+style churn.
- **Tab-bg sweep IS animated on main:** across busy phases, the busy tab's bg
  ramps per-column (#1e1e1e→#28…28 over 17 row-0 cells) — the tab_pulse.rs
  "envelopes stay C2" comment is STALE (sweep/glow envelopes landed). Ours-side
  animation confirmed at cell level.
- **Caret:** terminal-cursor based (no cell-phase blink) —
  `vt_temporal_cursor_blinks_500ms` PASS; idle byte-stability PASS.
- **Marquee — SA-F3 (S1):** hovered truncated title shifts every **~166 ms**
  (40 ms-poll gaps `[165,166,166,166,165,…]` after the 600 ms delay), not the
  pinned `MARQUEE_INTERVAL = 80 ms` (identical constant both trees:
  ours `components/marquee.rs:24`, ref `session-tabs.tsx:80`). The 80 ms drive
  source is quantized to the ~100 ms render tick → marquee scrolls at ~half
  speed. The standing `vt_temporal_marquee_shifts_one_cell_per_tick` FAILS
  (4 shifts < 5 in its 2 s window) — consistent with 166 ms.
- **Drive-source audit (code-cited):** spinner 80 ms/frame; run-loop tick
  100 ms; marquee 600/80 ms; caret = DECSCUR-class terminal cursor;
  StartupLoading 500/3000 ms (E2-owned); title_shimmer = time-driven blend
  (ref `title-shimmer.tsx:53-128`).

**Ref leg (fresh plugin-armed run, same rig env):** ref busy series advances
⠙→⠸→⠼→⠦→⠧→⠏ ≈ 1 glyph/120 ms sample (80 ms-class cadence — matches ours).
The burst-script fixture (StubScript::ReasoningLong paces nothing) cannot
adjudicate busy-window DURATION parity (I3: no stepping on that leg — ours
completes a burst in <120 ms where ref's pipeline renders for seconds; the
in-process vt-clock leg owns our-side busy behavior).

### SA-F4 (S2) — ref shimmer leg inconclusive (instrument design)
`title_shimmer` is time-driven, but the standing temporal scenario samples
AFTER typing settles → 0 style deltas across 60 ref frames (also true of the
2026-10-17 run, which recorded the same). Per I6, no parity claim from
settle-captures; carried with the instrument note (sample during typing).

### Instrument note (I6)
`cargo test --test vt_temporal` (all 5 in one process) self-poisons via the
60-second rule — libtest's "running for over 60 seconds" warnings get typed
into the app; 4/5 failed as a suite while each passes standalone. Run
individually.

---

## A5 — Plugin-rendered surfaces (incl. feature-plugin SLOT architecture)

**Census pin re-executed (2026-10-18):** `ls -1
/opt/opencode-ref/packages/tui/src/feature-plugins/` = exactly
{home, prompt, sidebar, system} ✓ (internal slots enumerated; the plugin-side
slot API cells below).

**I2/I2b rig:** omo-slim **3.0.4** (full npm stamp incl. hoisted deps,
`/tmp/opencode/scanab/omos-npm/stamp-full`, I9: the user's real cache copy) ×
ref **2.0.25 binary** (`--ref-bin`, executed copy) + ours @ lane-scanab, one
shared project, both plugin caches seeded. Rig env recorded
(`ref-live*/rig-env.txt`). Fresh captures:
`/tmp/opencode/scanab/ref-live{,2}/{session,temporal,plugindiag}`.

**I2b gate states observed:** incomplete-plugin-tree leg (ref-live, first
attempt): our TUI-entry load failed `Cannot find package 'jsonc-parser'` AND
ref's TUI sidebar section also absent (both-inactive — the gate condition
"ref plugin ≠ active while ours active" not violated; leg discarded as
evidence, re-run with the full tree). Full-tree leg (ref-live2): ref sidebar
section renders, ours renders — gate PASS.

**Result (side-by-side cell diff, session-07 both sides):**

| surface cell | ours | ref 2.0.25 + plugin |
|---|---|---|
| OMO-Slim section presence | ✓ renders (rescan **F1 fix verified live on main**) | ✓ |
| section rows | `• orchestrator default` (tight) | `• orchestrator` + padded col + `default` (wide columns) |
| row fg | **idx8 (dark gray)** | **idx15 (bright white)** |
| header fg | idx255 (#eeeeee-class) | idx15 (#ffffff-class) |
| version badge | **`v2.0.24`** (our app version — wrong source) | `v3.0.4` (the plugin's version) |

### SA-F5 (S1) — plugin sidebar rendering fidelity (ours host-rendered vs ref plugin-rendered)
The plugin's rows render near-invisible-class gray on ours (idx8) where ref
renders white (idx15); the header shade, the version badge source, and the
inter-column padding all diverge. Our host renders the plugin's DATA
(fix5's native surface) rather than the plugin's own renderer output — the
observable surface differs in readability and metadata. [G]-granularity:
per-cell evidence above; carries to C6 (plugin lifecycle × UI) for the
product decision (execute tui2.js vs shape the native surface to the plugin's
render contract).

### SA-F6 (S1) — `/plugins` live dialog: ref opens, ours types
Driver types `/plugins` (no Enter): ref opens the Plugins dialog overlay
("Plugins / Search / No items available / ctrl+a show internal"); ours leaves
the text in the composer with no live slash-command surface. (Command-
palette visibility was rescan F9's unasserted cell — now asserted DIVERGENT
for the live-typing phase.) C15/C20-adjacent; B1 records the key side.

### SA-F16 (S2) — plugin-host uncaughtException on unreadable dirs
During plugin health checks our host logged
`uncaughtException: EACCES /lost+found` and `EACCES /tmp/tmux-0` — contained
(logged, boot continues), but uncaught exceptions in the host from a plugin's
filesystem probing are a C5/C6 robustness cell (a plugin scanning
permission-denied paths should yield an error result, not an uncaught
throw). Evidence: `ref-live2/session/our-home/.local/state/opencode-rust/tui.log:2,4`.

### Instrument repair (META — SA-F15)
`rescan-tui` was **compile-broken on main**: 7557ae8 removed
`parity::tui_diff` while the rescan lane's driver (3b8700c) still imports it
(and `termatrix` + the vt100 dep went with it). This lane restored both
modules verbatim from `7557ae8^` and re-added `vt100 = "0.15"` (same pin as
the vt harness) — the I5-bound instrument builds and runs again. Needs
upstreaming (product code untouched).

---

## B1 — Keyboard end-behavior table

**I8 canary: RUN + PASS** (`b1_i8_canary_keybind_override`): `keybinds
{"app.exit": "ctrl+q"}` via the real config chain → default ctrl+c **dead**
(no teardown in 3 s) + custom ctrl+q **exits** — the end-behavior method
distinguishes bindings exactly.

**End-behavior table (live, artifact `/tmp/opencode/scanab/b/end-effects.md`):**

| route | state | input | end-effect | verdict |
|---|---|---|---|---|
| home | base | type `abc` | composer echo | echoed |
| home | empty draft | ctrl+c (`app.exit` default) | teardown | exits |
| home | base | `<leader>l` | session dialog lists Alpha | opens (A1-matrix drive) |
| dialog | open | ArrowDown | selection row 0→1 | moves (A1 dialog-selected-row1) |
| dialog | row selected | Enter | session tab opens | opens (A1 flow) |
| session | user-only transcript | `<leader>y` (messages.copy) | v2 miss toast, no OSC 52 | toast=shown (live) |
| session | last-assistant present | `<leader>y` | OSC 52 write | golden `messages_copy_resolves_text_and_toasts` PASS (I5) |
| (config) | leader.timeout=300 | `<leader>` +600ms + `l` | 'l' leaks to composer | CARRIED (early-exit restructure; the cell needs its own boot) |
| canary | app.exit rebind | ctrl+c / ctrl+q | default dead / custom exits | PASS |

Notes: dialog cells evidenced via the A1 matrix captures + `open_first_session`
asserts (SA-F13 forbids a second in-process boot after the ctrl+c-exit cell).
The leader-timeout cell is CARRIED (reason: test-order, evidence design ready).
SA-F14 (first chord ignored with a config FILE present) needs a dedicated
repro — the canary's config came via the env overlay and exited cleanly.

---

## B2 — Pointer real shapes

**Method:** SGR 1006 hit-inject (`sgr_press/release/right/middle/drag/scroll`)
+ observed grid effect. Hover-state cells (target × hover) were captured in
the A1 matrix (tab title, `+`, off-strip control). Artifact:
`/tmp/opencode/scanab/b/pointer-effects.md`.

| target | state | shape | end-effect |
|---|---|---|---|
| tab strip | two tabs, 2nd active | click tab 1 | **switches** ✓ |
| tab strip | N tabs | click `+` | **NO-TAB** (SA-F7) |
| transcript | base | right-click | no-op (observed) |
| transcript | base | middle-click | no-op (observed) |
| transcript | short (no overflow) | wheel down | no-op — fixture-bound disposition |
| tab strip | base | drag 3→20 | changes (repaint) |

### SA-F7 (S2) — `+` click creates no tab
The new-tab affordance highlights on hover (hover sweep) but a press+release
creates no tab on ours. Ref-side click behavior unevidenced (the standing
driver has no `+`-click leg) — carried for the ref pair before severity is
final. Selection-copy of RENDERED content (wide-char reconstruction class):
the OSC 52 channel is proven (B1); the wide-char/emoji/masked-text copy
corpus is CARRIED (needs a transcript fixture with such content).

---

## Execution ledger (A1–A9 + B1–B9)

Priority order per the lane brief (A1, A4, A5, B1, B2 first; then
A2/A3/A8/B3/B4/B5; then A6/A7/A9/B6/B7/B8/B9 as window allows).

| clause | status | artifact / carry reason |
|---|---|---|
| A1 | **EXECUTED** | canary PASS (2 legs) + state-matrix capture + ours-vs-ref cell diff (`spec_scanab_a1.rs`, `a1_cell_diff.py`, `/tmp/opencode/scanab/a1/`); states error-row (ours-golden-only), disabled/expanded/interrupted CARRIED within A1 |
| A4 | **EXECUTED** | vt_temporal standalone runs + marquee cadence probe + fresh ref busy/shimmer series; ref shimmer leg inconclusive (SA-F4); burst-script busy-duration non-comparable (I3 fixture) |
| A5 | **EXECUTED** | fresh plugin-armed ref(2.0.25)+ours run with full plugin tree, census pin re-run, side-by-side sidebar/dialog diff (SA-F5/F6/F16); ≥3-ecosystem breadth CARRIED (omo-slim only this lane; marketplace fetch + 2 more ecosystems need rig extension) |
| B1 | **EXECUTED** | canary PASS + end-behavior table (live cells + golden-cited copy path); leader-timeout cell CARRIED (restructure); ref-side key traces CARRIED |
| B2 | **EXECUTED** | pointer end-effect table (click/right/middle/wheel/drag) + hover cells from A1; wide-char copy corpus + rect-lifetime-across-route-change CARRIED; `+`-click ref pair CARRIED |
| A2 | CARRIED | geometry golden-frame width sweep not started — window exhausted by the five priority clauses + instrument repair (SA-F15) |
| A3 | CARRIED | edge/huge-data corpus (0/1/2/10/60/80/120/240 × part-kind × wide-char × overflow) — needs its own lane window |
| A6 | CARRIED | MINI frontend panel matrix — needs mini PTY rig time |
| A7 | CARRIED | theme-system chain — partially touched (partial-override docs invalid; OSC-11 mode detect untested); full chain round-trip pending |
| A8 | CARRIED (census verified) | ours-side absence re-verified mechanically (`grep -ric mermaid|merman|latex|katex crates/` = 0 nonzero, 2026-10-18); ref registrations re-verified (merman plugin.ts:8, latex plugin.ts:11-12); renderer corpus diff not run |
| A9 | CARRIED | images family untouched |
| B3 | CARRIED | focus/layering stacks — SA-F13/F14 feed it (exit-chord layering observations recorded) |
| B4 | CARRIED | paste/IME/OSC-52-read byte traces — OSC-52 write proven (B1); read/paste forms pending |
| B5 | CARRIED | terminal-event scripts pending |
| B6 | CARRIED | color-count fallback flagged by SA-F1 residual (ssh-rule ref leg); emulator matrix not started |
| B7 | CARRIED | env matrix not started |
| B8 | CARRIED | width corpus not started (canary design exists in the matrix) |
| B9 | CARRIED | remote/hosting matrix not started |

**Counts: EXECUTED 5 / CARRIED 13** (A8 additionally census-verified).

## Findings index

| ID | axis | severity | one-line |
|---|---|---|---|
| SA-F1 | A1/B6 | S2 (residual) | ssh⇒truecolor probe rule: ours verified live; ref-side live check CARRIED; matched-env + quantizer parity both PASS |
| SA-F2 | A1 | closed | sidebar Context/tokens/usage cells: exact parity (i255/i8 both sides) |
| SA-F3 | A4 | **S1** | marquee effective cadence ~166 ms vs pinned 80 ms (render-tick quantization) |
| SA-F4 | A4 | S2 | ref shimmer leg inconclusive (settle-sampling instrument design) |
| SA-F5 | A5 | **S1** | plugin sidebar fidelity: gray rows vs white, badge v2.0.24 vs v3.0.4, padding collapsed |
| SA-F6 | A5/B1 | **S1** | `/plugins` live dialog on ref vs plain composer text on ours |
| SA-F7 | B2 | S2 | `+` click creates no tab (ref pair pending) |
| SA-F8 | B1/B2 | positive | keybind-override canary, copy miss-toast parity, OSC52 golden, tab-click switching |
| SA-F13 | tooling | **S1** | in-process double boot: teardown hang + dead chord (embedders/tests) |
| SA-F14 | B1/E9 | S2 | first exit chord sometimes ignored with a config FILE present |
| SA-F15 | META | instrument | rescan-tui compile-broken on main (tui_diff removal vs driver import) — repaired this lane, needs upstreaming |
| SA-F16 | C5/C6 | S2 | plugin-host uncaughtException on EACCES dirs during health checks |
