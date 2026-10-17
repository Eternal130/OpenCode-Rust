# SPEC-R7 — Round-7: CONVERGENCE (P3 environment/matrix + P4 process-fidelity + P5 granularity sweep; v6 integration)

**Lane:** SPEC-R7 (worktree `.worktrees/lane-spec7`, branch `lane/spec7`).
**Date:** 2026-10-17. **Inputs:** `docs/scan-spec.md` v5 (appendix 114/114,
canaries green), `docs/spec-round6.md` §6 (P3 directives), `docs/rescan-findings.md`
(the field-instrument lesson). **Output:** `docs/scan-spec.md` **v6** (appendix
129/129), this report. P1/P2 CLOSED (R4/R6).

## §1 P3-D1 — ref-runtime provisioning: RESOLVED, with measurements

### §1a What the blocker actually was (R6's diagnosis corrected)

R6 §3 recorded the blocker as "full ref TUI boot requires the complete bun
dependency tree … Provisioning = full `bun install`". That diagnosis was
**wrong in its cause, right in its symptom**:

1. `bun install --frozen-lockfile` in `/tmp/opencode/ref` completes in
   **4.45 s, "Checked 2504 installs across 2934 packages (no changes)"** —
   the lockfile store was ALREADY complete (the tui-dump lane's selective
   install had populated `node_modules/.bun` for all 2934 packages). No
   dependency was missing.
2. The boot failure (`Cannot find module 'react/jsx-dev-runtime'`) is a
   **launch-path bug, not a dependency gap**: `packages/tui/src/index.tsx`
   is a LIBRARY (`export { run, type TuiInput }`); the launcher is
   `packages/cli` (`bun run dev` → `src/index.ts` → `default.ts` handler →
   `Tui.run`). Bun resolves the per-package tsconfig
   (`jsxImportSource: "@opentui/solid"`) from **CWD** — launching
   `bun packages/tui/src/index.tsx` from the repo root applies the ROOT
   tsconfig, whose default JSX runtime is react → the module error.
3. **Acceptance:** ref TUI boots in a PTY from source — VERIFIED (12 KB of
   frames, boot status line `✓ Server  ○ UI …`, session UI + composer echo
   within seconds; see §1c). The old `tui-dump` export.ts determinism gate
   no longer exists on this machine (`/tmp/opencode/tui-dump/` cleaned);
   boot-stage determinism is instead evidenced by the N=10 legs below
   (session-screen time 5.0–5.1 s across all 10 source runs; frame-byte
   counts within 10.1–11.1 KB).

### §1b Managed-service port discipline (the environment trap)

The ref CLI spawns a **detached background server** ("Starting background
server…") that survives `killpg` of the TUI. The managed-service port is
fixed per install tree: **49374** = the user's LIVE 2.0.25 service (do not
touch), **49375** = the source tree's default. Probing discipline (recorded
as the clause method): per-run sandbox HOME + `opencode service set port
<49500+idx>` + post-run kill of the detached server by port. Without this,
run 2+ of any N-run series dies with "Managed service port … already in
use" — this exact collision killed our-binary TTI runs 1–9 (§1d).

### §1c Measured: ref TTI baselines (E2), N=10 per leg, same PTY probe

Marker per E2: T0 = process spawn; T_interactive = first frame where the
session composer accepts+echoes a probe key ("z"; poll every 2 s after the
session screen paints, echo = new visible 'z'). Same host, sequential runs,
fresh sandbox HOME per run.

| Leg | T_session (median) | T_interactive median | p95 | max | N |
|---|---|---|---|---|---|
| ref **source-dev** (2.0.24 tree, `bun run dev` from `packages/cli`) | 5 050 ms | **5 250 ms** | 5 300 ms | 5 300 ms | 10 |
| ref **binary** (2.0.25, `/root/.opencode/bin/opencode` — the I2b ground truth) | 1 450 ms | **2 300 ms** | 2 400 ms | 2 400 ms | 10 |
| ours (R6, harness-attach T0 — NOT ratio-eligible, see E2 amendment) | — | 569 ms | 571 ms | 571 ms | 10 |

Raw series + captures: `tools/parity/spec-p3/ref_tti_{0..9}.ansi`,
`refbin_tti_{0..9}.ansi`, `ref_tti_results.json`, `refbin_tti_results.json`
(summaries committed; full ANSI kept under `/tmp/opencode/spec-r7/`).

### §1d Ours process-spawn leg: attempted, not landed (honest record)

`ours_tti_probe.py` ran 10 legs: run 0 booted fully (21.7 KB frames, OSC-11
query + paint) but raw-ANSI regex cannot see our home screen (needs the
vt-grid mirror, not stripped-byte matching); runs 1–9 died on the fixed-port
server collision (§1b — our binary spawns a server the same way). Standing
recipe committed (`tools/parity/spec-p3/ours_tti_probe.py` +
`ours_tti_run1_portcollision.ansi`); the ours process-spawn TTI number
remains a **P3 field-run item** (the R6 harness-attach number 569 ms stands,
with its T0 caveat). The E2 ratio assert is therefore formula-ratified +
ref-values-measured, ours-ratio-leg pending — the R6 "blocked" row for ref
TTI is now **filled** (both ref legs), which is what P3-D1 asked for.

### §1e Ref E3/E10/E11 soak baselines

≥4 h runs exceed the ≤45-min round budget → deferred to the field run with
the recipe now fully specified: the ref process boots (§1c), the E3 sampler
is canary-validated (R6 §2b), and the port/kill discipline (§1b) removes the
last environment trap. No spec gap remains — execution debt only.

## §2 P3 track — B6/B7/B8/B9 to executability (the v6 riders, verbatim)

Tool availability measured this round: tmux 3.4 ✓ · ssh + sshd(active) ✓ ·
docker(daemon) ✓ · tc ✓ (root) · screen ✗ · mosh ✗ · socat ✗ ·
xterm/kitty/ghostty/wezterm ✗ (no local X/virtual display). Every rider
below appends to (does not replace) the clause's v5 text.

**B6 rider:** Executability table (R7): per B6 cell the runner emits
{tool, threshold, environment, disposition}. Tool = standing
protocol-signature probe script (extend the TM-1..10 harness) + behavior
capture pair (ours vs ref in the SAME emulator). Threshold = (i) signature
cells: exact response-byte equality for the query set; (ii) behavior cells:
frame parity ours-vs-ref at equal window size in that emulator; a
class-level diff (not byte-diff) for animation cells per the I6 masking
rule. Environment dispositions (measured 2026-10-17): LIVE = tmux x{1,2},
no-TERM, 16-color, ssh (loopback sshd); DOCKER = screen, xterm-under-Xvfb
(container provides Xvfb); DEFERRED-WITH-RECIPE = kitty, ghostty/wezterm-class
(container image per emulator + vt-dump compare; headless-render capability
named per emulator before the cell may close). Artifact = the cell table
with disposition column (executed / docker / deferred-recipe), committed
per round. {R7: G3}

**B7 rider:** Executability table (R7): env-cell runner = boot+smoke under
`env(1)` per cell; assert = boot reaches first frame AND one smoke action
completes; diff-vs-baseline-class table per cell. Environment dispositions
(measured 2026-10-17): LIVE = unset TERM, no HOME, no PATH, no git/rg,
read-only HOME, LANG x {C, en_US.UTF-8, non-UTF-8}, TZ absent, OPENCODE_*
incl. CLI_CONFIG_CONTENT, SSH_TTY, CI vars, docker; DEFERRED-WITH-RECIPE =
windows/ConPTY (needs a real Windows host — wine does not reproduce ConPTY
semantics; recipe = CI windows runner + the same runner script), macOS
(recipe = CI macOS runner); sound-backend-absence and per-OS named cells
ride their OS disposition. Artifact = matrix JSON + per-cell PASS/FAIL.
{R7: G4}

**B8 rider:** Executability (R7): tool = committed corpus runner that
renders each corpus string through both width implementations and compares
**cursor-cell position** (the clause's pass criterion); corpus = the
codepoint-class set of the clause, checked in (extends A3's wide-char set).
Threshold = zero desync on the unperturbed corpus, per class; the I8
canary (width-table perturbation desyncs EXACTLY the perturbed classes)
runs BEFORE the first B8 evidence is accepted (designed, not yet run — no
B8 evidence exists yet, so the I8 tier order is intact). Artifact = per-
class cursor-position table ours vs ref. {R7: G5}

**B9 rider:** Executability table (R7): per B9 cell {tool, threshold,
disposition}. Environment dispositions (measured 2026-10-17): LIVE = ssh
(loopback sshd), tmux-over-ssh, container (docker daemon OK), CI env vars,
dumb-tty (TERM=dumb), shaped link (tc, root: 100 ms+ RTT latency + loss);
DOCKER/RECIPE = mosh (mosh MISSING on host; container pair mosh-client/
mosh-server over UDP). Threshold = per cell: boot-succeeds under the
degraded env + echo-latency matrix (send→echo p50/p95 under each shaped
RTT, ours vs ref same shaping) + redraw-storm ceiling (frames-per-second
under a full-redraw trigger, ours ≤ ref ×2); read-only-HOME/no-/dev/pts
cells assert the documented failure-or-degrade behavior, not silence.
Artifact = cell table + latency CSV per shaped profile. {R7: G6}

## §3 P4 track — I-layer grounding + canary matrix + masking rule

### §3a I-layer empirical-grounding audit (all clauses I1–I11)

| Clause | Grounded by (evidence) | Verdict |
|---|---|---|
| I1 real payloads | vt_harness FakeLlm chains; rescan-tui.rs live plugin flows | grounded |
| I2 real sandbox | RESCAN plugin-armed both stacks (omo-slim 3.0.3); rig gap = I2b's cause | grounded + amended (I2b) |
| I3 virtual clock | E1 canary drive audit (~107 ms tick); R6 pins verified at ref source | grounded |
| I4 token-exact cells | E5-a canary on real resolved-token pairs | grounded |
| I5 bound instruments | golden_tests.rs, frame_budget_tests.rs, replay.rs, mouse_modes.rs, diff_viewer_golden_tests.rs, simulation tapes + R6/R7 additions (this round's amendment) | grounded + extended |
| I6 miss-attribution ledger | RESCAN §1-§3 attributions; masking audit §4 = the new standing rule | grounded + amended |
| I7 enumeration backstop | quantified vs I10b universe (35 pkgs, verified 2026-10-17 R3 sweep; R4 re-sweep) | grounded |
| I8 tiered canary | R6 5 legs green; coverage matrix (§3b) | grounded + matrix added |
| I9 provenance | E12 corpus I9-labeled; E4 generator I9-labeled | grounded |
| I10 ground-truth pin | ref commit pinned; now TWO ref forms (source 2.0.24 / binary 2.0.25) → I2b records the split | grounded + amended |
| I10b universe pin | 35-package list + standing OOC entries | grounded |
| I11 process contract | three lossy integrations cured (v4); v5/v6 verbatim + zero-loss scripts | grounded |

No I-layer clause is nominal. Two amendments (I5, I8) + one new clause
(I2b) + one rule fold (I6) — all verbatim below.

### §3b Canary-coverage matrix (I8 amendment, verbatim)

I8 rider (v6): **R7 canary-coverage matrix (v6),
   maintained per round** (a standing artifact under I5): RUN = canary
   executed, PASS/FAIL recorded; DESIGNED = executable design bound, runs
   before that axis's first evidence; NONE-DUE = axis has no evidence yet
   (canary due at first evidence per the tier rule); OVERDUE = evidence
   exists without an axis canary — must clear before further evidence
   counts. Matrix at R7 close: RUN (5 legs, PASS) = E1, E3, E5; DESIGNED =
   A1, B8, C4, D1, F7 (R3 §3 designs) + E2 (inject +250 ms one-shot delay
   into a throwaway ours boot path, assert the measured T_interactive
   shifts by ≥250 ms in that copy only — run before the E2 ratio verdict)
   + E12 (poison one corpus cell — flip a matching string to non-matching
   — assert `spec_e12_first_pass` flags exactly that query; binding before
   the second pass, per R6 §6.3); all other axes NONE-DUE. Defer-or-design
   rule (standing): an axis taking evidence MUST have its canary RUN first
   (tier rule — unchanged); axes not yet taking evidence do not pre-build
   canaries (designs land with the axis's method, as E2/E12 did in R7).
   {R7: G9}

| Axis | State | Basis |
|---|---|---|
| E1, E3, E5 | RUN (5 legs, PASS) | R6 §2/§2b; corpus committed |
| A1, B8, C4, D1, F7 | DESIGNED (R3 §3 designs bound) | run at first evidence |
| E2 | DESIGNED (this round) + OVERDUE-cleared-on-design | TTI numbers R6/R7 = baseline artifacts (no comparison verdict emitted yet); design: inject +250 ms one-shot delay into a throwaway ours boot path, assert the measured T_interactive shifts by ≥250 ms in that copy only (probe = spec-p3 TTI probes; run before the E2 ratio verdict) |
| E12 | DESIGNED (this round) + binding-before-second-pass | first pass R6 was ours-side corpus parity; design: poison one corpus cell (flip a matching string to non-matching), assert `spec_e12_first_pass` flags exactly that query; R6 §6.3's second pass may not count until this runs |
| all other axes (A2-A9, B1-B7, B9, C1-C3, C5-C21, D2-D11, E4, E6-E11, F1-F6) | NONE-DUE | no axis evidence recorded yet |

Defer-or-design rule (standing): an axis taking evidence MUST have its
canary RUN first (I8 tier rule — unchanged); axes not yet taking evidence
do not pre-build canaries (designs land with the axis's method, as E2/E12
did this round). {R7: G9}

### §3c I6 masking-rule fold (verbatim, from RESCAN §4/§5-10/§6-5)

I6 rider (v6): **Umbrella-adjudication masking rule (standing; applies to
ANY comparison instrument, current or future)**: an umbrella (b)-class
(coarse mismatch classes — e.g. coin-flip placeholder, digit drift, indexed
color artifact) may NOT absorb a step that contains animated/busy/indicator
cells; steps containing such cells require per-cell family classification.
A closure may not rest on identical mismatch counts versus a baseline
("19 == 19") — it requires per-cell family evidence. Settle-then-capture
discipline may never be cited for animated-element parity (what the capture
never observed cannot be adjudicated equal). Evidence base: RESCAN §4
(41 corpus steps absorbed across 2 scripts; the "no new spinner class"
conclusion was false at the element-family level — RESCAN F3 proved a real
busy-indicator divergence the umbrella had swallowed). {R7: G7}

### §3d I5 instrument extension (verbatim)

I5 rider (v6): the list additionally
   binds (R6/R7): `crates/oc-tui/tests/spec_e1_canary.rs`,
   `spec_e5_canary.rs`, `spec_e2_tti.rs`, `spec_e12_first_pass.rs` (+ corpus
   `tools/parity/spec-canaries/e12_search_corpus.json`),
   `tools/parity/spec-canaries/e3_leak_canary.rs`,
   `tools/parity/src/bin/rescan-tui.rs` (the plugin-armed live-capture
   driver — the ONLY standing instrument proven to load the current plugin
   generation on both stacks), and `tools/parity/spec-p3/` (ref/refbin/ours
   TTI PTY probes + results). Rounds must not rebuild ad-hoc variants of
   these. {R7: G8}

### §3e I2b — the ref-binary ground-truth clause (verbatim, new)

I2b **Plugin-era ground-truth binary**: for every plugin-surface comparison
(I2's ref+plugin pairing, A5, C6, D3, and any clause touching plugin-
rendered or plugin-registered content), the ground-truth ref = the **ref
2.0.25 binary** (the user's `/root/.opencode/bin/opencode`) + the current
plugin (omo-slim first), NOT the pinned 2.0.24 source tree: 2.0.24
**cannot load the current plugin generation** (`Cannot find module
'oh-my-opencode-slim/rpc'`; plugin shows failed in /plugins; evidence:
docs/rescan-findings.md §3-F10, ref-home log err_7b5efd85…d5244f41) — any
plugin-era parity claim differenced against 2.0.24 is **vacuously true**
(both sides broken ⇒ no divergence visible). Standing rule: every
plugin-era comparison records which ref form it used (source-2.0.24 for
non-plugin source parity; binary-2.0.25 for plugin-era), and an instrument
gate FAILS when the ref's plugin state ≠ active while ours is active (and
vice versa). {R7: G1, G2; evidence: RESCAN F10 + §1b/§1c of
docs/spec-round7.md}

### §3f E2 legs amendment (verbatim)

E2 rider (v6): the ref
   baseline is TWO legs, both N=10, same PTY probe: (i) **source-dev
   leg** (I10-pinned 2.0.24 tree, `bun run dev` from `packages/cli`;
   dev-transpile overhead included — recorded as the source-parity
   leg), (ii) **binary leg** (I2b 2.0.25 user binary — the
   user-comparable leg). The ×1.5 ratio gates on the **binary leg**; the
   source leg is recorded for drift detection. Ours T0 for the ratio =
   process spawn (harness-attach numbers are recorded separately and are
   never ratio-eligible). Measured (2026-10-17, R7): ref source-dev
   T_interactive median **5250 ms** (p95 5300), ref binary median
   **2300 ms** (p95 2400); ours process-spawn leg pending (field run;
   recipe committed in `tools/parity/spec-p3/`). {R7: G15}

## §4 P5 track — granularity sweep: stratified-random 15, letter-vs-spirit

Sampling: stratified per axis (I2 A2 B2 C3 D2 E2 F2), seeded shuffle
(`random.Random(7)`, reproducible) → **A2 A3 B4 B6 C1 C4 C6 D11 D3 E4 E7
F1 F7 I11 I8**. B6 and I8 were stressed-and-rewritten by the P3/P4 tracks
(§2, §3) — counted here, not double-rewritten. Verdicts:

| Clause | Verdict | Gap |
|---|---|---|
| A2 geometry | WEAK — no threshold, artifact unnamed, "rule-by-rule" unbounded → REWRITE | R7-G10 |
| A3 edge+huge | PASS (corpus persisted, bound harness, cell table artifact) | — |
| B4 input methods | WEAK — no threshold on the byte-trace, no artifact → REWRITE | R7-G11 |
| B6 terminal matrix | REWRITTEN (P3 rider, §2) | R7-G3 |
| C1 session lifecycle | PASS (table-driven both ends, states from ref code) | — |
| C4 config leaf keys | PASS (per-leaf round-trip assert, restart, cross-install) | — |
| C6 plugin lifecycle | WEAK — "drive + capture" with no state assertions/no parity target → REWRITE | R7-G12 |
| D3 plugin-host boundary | WEAK — trace with no comparison oracle; post-RESCAN must disposition absent APIs as findings → REWRITE | R7-G13 |
| D11 SDK/client | WEAK — "shape-compat assert" with no tool/threshold/artifact → REWRITE | R7-G14 |
| E4 huge-data | PASS (marker + N=20 + ×1.5 + p95 table) | — |
| E7 error quality | PASS (copy-diff + placeholder scan + exit-code table) | — |
| F1 materialized matrix | PASS (coverage % + ledger + I8 canary on matrix) | — |
| F7 negative-space | PASS (scope + granularity + disposition rule + canary + pre-dispositions) | — |
| I11 process contract | PASS (this round is itself the corrected-instance practice) | — |
| I8 tiered canary | REWRITTEN (matrix amendment, §3b) | R7-G9 |

8 PASS · 5 REWRITE · 2 rewritten-by-track. The 5 rewrites (verbatim, = the
v6 text):

**A2 (v6 rewrite):** Geometry (padding/gap/centering/truncation/wrap). [G] per surface
   x size {40,60,80,120,160} + narrow/wide sweep [F] vt grid golden-frame
   capture [M] golden-frame diff at width sweep, rule-by-rule vs ref;
   **threshold = zero unexpected cell diffs per surface x width cell —
   expected-diff classes are enumerated and dispositioned per cell
   (glyph-width ambiguity is B8-owned; theme-token deltas are A1-owned;
   anything else is a finding)**; artifact = per-surface x width diff
   table + the golden corpus committed in-repo. {R7: G10}

**B4 (v6 rewrite):** Input methods: [G] per paste form (bracketed/image/large/WHATWG), IME
   composition, clipboard read/write (OSC 52) x size (incl. 100MB) [F] real
   buffers/paste payloads [M] inject + observe; byte-level paste/clip
   traces; **threshold = byte-identical round-trip per form (bytes in =
   bytes observed in the app's input buffer, modulo a documented
   terminal-normalization class list: CRLF↔LF, bracket markers stripped by
   the terminal, pasted-newline policy) — every normalization ours applies
   that ref does not is a finding; OSC-52 write lands in the system
   clipboard (asserted via clipboard readback tool) and read returns the
   planted content; IME = preedit→commit sequence observed in the composer
   trace**; artifact = per-form byte-trace pair (ours vs ref) +
   normalization-class diff table. {R7: G11}

**C6 (v6 rewrite):** Plugin lifecycle x UI: [G] install/enable/disable/update/fail x UI x
   storage/RPC (PluginStorage, NoopRpcPort) x phase; feature-plugin SLOT
   architecture slots are surfaces (R3 fold 5). [F] I2 real plugins, real
   payloads [M] lifecycle drive + capture, **with a three-layer state
   assert per lifecycle op: (1) plugin-state API/UI cell, (2) storage
   write (PluginStorage keys), (3) RPC/registration effects — and parity
   differenced against the I2b ref-binary leg (2.0.25 + plugin); each
   layer divergence per op x phase is a finding, not a capture-only
   record**; artifact = per-op x phase state-transition table ours vs ref
   (I2b). {R1: S1-11; R2: (a)S1-11; R3: FOLD-5; R7: G12}

**D3 (v6 rewrite):** Plugin-host boundary: [G] per API surface x real payload (omo-slim + ≥2
   other ecosystems) [F] I2 [M] host-adapter taps, boundary trace —
   **compared against the I2b ref-binary leg with a per-call disposition
   column: {parity | absent-API-on-ours (finding, F1/F7-class) |
   plugin-never-calls (justified)}; a trace with no comparison oracle is
   not evidence; an API the plugin calls on ref and cannot call on ours
   is a FINDING by definition (the vacuous-truth hazard I2b exists
   for)**. Artifact = per-surface call matrix (args + response shapes)
   with the disposition column. {R1: S3-4; R7: G13}

**D11 (v6 rewrite):** **SDK/client package** (RESTORED v4 — R2's §c-D10, silently vanished in
   v3; R3 left the fold decision to R4 -> CLAUSE): [G] public SDK + client
   shapes the app/SDK consumers rely on x consumer {ref app, external} —
   versioned [F] I10 versioned pins [M] **shape extraction = type-level
   export diff per pinned version pair (public entrypoints: named
   exports, function signatures, type aliases — via the ref's own type
   tooling against `packages/sdk`/`packages/client` .d.ts); threshold =
   zero un-dispositioned breaking changes between consecutive pinned
   versions (breaking = removed/renamed export or widened signature;
   additive changes recorded, non-blocking)**; artifact = per-version-pair
   diff report committed with the round. {R2: §c-D10; R3: §5-nonP1; R7: G14}

## §5 v6 integration + zero-loss re-check

`docs/scan-spec.md` v5 → v6: 1 new clause (I2b); 4 riders (B6, B7, B8,
B9); 5 P5 rewrites (A2, B4, C6, D3, D11); 4 amendments (E2 legs, I5
instruments, I6 masking rule, I8 canary matrix); header/lineage/round-log
updated; appendix +15 rows (R7-G1..G15) = **129/129**. Zero-loss re-check
(scripted, R6-style): clause census v5 81 → v6 82 (added I2b, dropped 0);
appendix census 114 + 15 = 129; all 15 R7 gap IDs mapped; verbatim
containment of the 11 R7 blocks (I2b, B6r, B7r, B8r, B9r, E2r, I5r, I6r,
I8r, and the 5 rewrites as 5 blocks) character-exact modulo whitespace —
**PASS** (script `tools/parity/spec-p3/zero_loss_r7.py`, output §5a of the
round log).

## §6 Convergence verdict — the FINAL-readiness call

- **P3 CLOSED**: B6/B7/B8/B9 executable (tool/threshold/disposition per
  environment, availability measured this round); P3-D1 resolved WITH
  measurements (ref TTI both legs; soak recipe complete, execution debt
  only); the ref-binary amendment (I2b) closes the vacuous-truth hazard.
  Remaining deferred cells all carry named recipes = specification-complete,
  execution-pending.
- **P4 CLOSED**: every I-layer clause empirically grounded (§3a audit
  table); canary-coverage matrix standing (5 RUN green, 7 DESIGNED with
  binding run-order rules, rest NONE-DUE); masking lesson folded as a
  standing any-instrument rule (I6 rider).
- **P5 CLOSED**: stratified-random 15 stressed; 8 PASS, 5 rewrites, 2
  by-track; no clause left with letter-but-not-spirit executability in the
  sample; the sample's axis spread covers every axis family.
- **P1 (R4), P2 (R6) already CLOSED.**

**CALL: the spec is FINAL-ready — v6 is declared the FINAL candidate.**
All five perspectives have converged (each track's round found no material
unowned gap; this round's findings were amendments and rewrites, all
integrated). Per docs/scan-program.md: next rounds are **RATIFY-1** and
**RATIFY-2** (full five-stance fresh-agent reviews of the FINAL candidate;
two clean rounds → LOCK).

## Round log (this lane)
- P3-D1: blocker re-diagnosed (launch-path, not deps) + resolved; ref TTI
  measured BOTH legs N=10 (source-dev 5 250 ms / binary 2 300 ms median);
  port+detached-server discipline recorded; harness committed
  (`tools/parity/spec-p3/`). Ours process-spawn leg attempted, pending
  (recipe committed).
- P3: B6/B7/B8/B9 riders to executability with measured tool availability.
- P4: I-layer audit 12/12 grounded; canary matrix (RUN/DESIGNED/NONE-DUE/
  OVERDUE semantics); E2+E12 canary designs bound; I6 masking rule; I5
  extension; I2b clause.
- P5: stratified-random (seed 7) 15-clause stress — 8 PASS, 5 rewrites.
- Integration: v6, appendix 129/129, zero-loss PASS.
- Verdict: P3/P4/P5 CLOSED → **FINAL candidate**; next RATIFY-1/RATIFY-2.
