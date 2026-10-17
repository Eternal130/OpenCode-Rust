# RATIFY-1 — First ratification round (post-FINAL-candidate, per docs/scan-program.md)

- **Under review**: docs/scan-spec.md **v6** (891 lines; appendix 129/129; P1-P5 declared
  CLOSED in docs/spec-round7.md; FINAL candidate).
- **Lane**: `lane/ratify1` (worktree `.worktrees/lane-ratify1`). Stances S1-S5 + META,
  one full pass each, fresh eyes on v6. No clause edits applied (ratification judges;
  proposals below are for R8 if the verdict is FAIL — it is).
- **Verdict summary**: S1 MATERIAL-GAP · S2 CLEAN (2 non-blocking residuals) ·
  S3 CLEAN · S4 CLEAN (2 residuals below the doc's own R7 rider standard) ·
  S5 CLEAN · META MATERIAL-GAP.
  **Overall: RATIFY-1 FAIL** → R8 integration, convergence counter resets.
  Two material gaps: **MG-1 unowned telemetry/observability family + false F7
  pre-disposition**; **MG-2 I10b enumeration-universe pin omits `plugin-browser`
  (claims 35, lists 34)**.

---

## Method + environment facts (measured this round, 2026-10-17)

- Ref tree `/opt/opencode-ref/packages/` = **35 packages** (ls census); ref binary
  `/root/.opencode/bin/opencode` present; all **I5-bound instruments exist** in our
  tree (verified file-by-file: `spec_e1_canary.rs`, `spec_e5_canary.rs`,
  `spec_e2_tti.rs`, `spec_e12_first_pass.rs`, `e12_search_corpus.json`,
  `e3_leak_canary.rs`, `rescan-tui.rs`, `golden_tests.rs`,
  `frame_budget_tests.rs`, `replay.rs`, `mouse_modes.rs`,
  `diff_viewer_golden_tests.rs`, ref `packages/simulation`, `tools/parity/spec-p3/`).
- S1 package selection by mention-census across round docs + spec (least-touched):
  effect-drizzle-sqlite (2), httpapi-codegen (2), http-recorder (2), storybook (2),
  util (2), containers (2), gui-extensions (3), console (5).
- S5 sample: `random.Random(11).sample` over the 82-clause universe (document
  order) → **D6, E9, D8, F7, E3, F1, B3, B2, F3, D9** (reproducible).

---

## S1 — feature-space (bottom-up, 8 least-deep-dived packages)

**Verdict: MATERIAL-GAP (MG-1).**

Package-by-package (bottom-up census of each, consumer graph via repo-wide grep):

| Package | Reality | Owned? |
|---|---|---|
| console | src empty (verified) | ✓ I10b accounting entry (accurate) |
| storybook | no src (package.json/playwright/sst only) | ✓ accounting class |
| containers | build recipes (base/bun-node/publish/rust/tauri-linux) | ✓ accounting; rides B7/B9 docker cells |
| httpapi-codegen | build-time codegen from effect httpapi schema | ✓ build tool; generated surface = D2's endpoint parity |
| effect-drizzle-sqlite | **no src at all** (examples only); sole importer = core TEST | ✓ accounting class |
| http-recorder | 14-file record/replay+redaction lib; sole consumers = `ai` TEST harness | ✓ accounting class (dev tooling) |
| util | 44 files: flock→**F3 names it**; session-title-fallback→**C21 names it**; retry/bom/patch/encode absorbed via I1 real chains; **observability → GAP (MG-1)** | ⚠️ |
| gui-extensions | **806 files** — app extension-UI library (file browser/tree, i18n catalogs da/fr/sv/zh/…); consumers = `packages/app` | ✓ via D6(U10) app disposition + F2 family accounting — transitivity implicit, noted not material |

### MG-1 — telemetry/observability family is UNOWNED, and F7's pre-registered
disposition (R5-Q12, v5) rests on a false product-level premise

Evidence (all fresh, this round):

1. **Ref wires OTLP observability in PRODUCTION, unconditionally, at CLI boot**:
   `packages/cli/src/index.ts:8` imports `Observability` from
   `@opencode/util/observability`; `cli/src/index.ts:126` provides
   `Observability.layer({endpoint: OTEL_EXPORTER_OTLP_ENDPOINT, headers:
   OTEL_EXPORTER_OTLP_HEADERS, client/version/channel…})` into the main effect.
   `packages/util/src/observability.ts` builds loggers from
   `Otlp.loggers(...)` + `Otlp.tracingLayer(...)` (OTLP JSON over
   `FetchHttpClient`, `OtlpExporter.layerFlusher`) and reads
   `OTEL_EXPORTER_OTLP_ENDPOINT`, `OTEL_EXPORTER_OTLP_HEADERS`,
   `OTEL_RESOURCE_ATTRIBUTES` (`util/src/observability/otlp.ts`). Emission is
   endpoint-gated; the layer + env reads + always-on structured logging are not.
2. **R5's adjudication examined core only**: docs/spec-round5.md:221 —
   "`openTelemetry` = recognized config key only, zero OTel implementation **in
   core** … NO client POST found" — the client-side wiring in `cli/src/index.ts`
   was never seen (a POST grep can't find env-gated OTel layer wiring).
3. **F7's standing pre-disposition inherits the false premise**: scan-spec F7
   (R5-Q12): "ref-side `openTelemetry` config key (recognized in
   `config/normalize.ts`, **no implementation in ref core**…)". Letter-true for
   `packages/core`; false for the ref **product binary** the scans actually run.
   Load-bearing both directions: if ours ever adds OTLP, the disposition brands
   an ours env-read/network row a false positive; as-is, the ref-only family is
   invisible to F7's ours-minus-ref letter.
4. **Live divergence today**: our tree has zero `OTEL_/otlp` matches (grep across
   `crates/`) — ref reads 3 OTEL_* env vars at boot and can emit OTLP HTTP with
   user-supplied headers; ours does nothing. No clause owns this family's parity:
   B7 names `OPENCODE_*` only (no OTEL_*); C4 owns only the config-key leaf
   round-trip; U8's OOC entry covers `stats/` (server-side site), not client
   telemetry; E8's SSRF targets are fetch/websearch; E10/E11 would at best see
   incidental magnitude effects. This is exactly the U-family class (a de-facto
   **U13**) that R3 existed to catch, plus an I6-class masking premise.

**Proposed cure (for R8; NOT applied):** (a) new family clause (e.g. C22 or
F-layer): telemetry/observability parity — OTEL_* env-read set, openTelemetry
config-key behavior (on/off/endpoint/headers), OTLP log+trace emission
(env-gated, mock-collector assert), always-on log-file layer (channel/version
attrs), dispositioned against I2b binary leg; (b) add `OTEL_*` cells to B7's
env list; (c) REWRITE the F7 R5-Q12 pre-disposition: "implementation lives in
`packages/util/observability`, wired unconditionally in `cli/src/index.ts`
(boot layer; emission gated on OTEL_EXPORTER_OTLP_ENDPOINT); core has none";
(d) I10b/F2 accounting entry for the family.

---

## S2 — quality (E-clause executability: artifact + threshold per clause)

**Verdict: CLEAN** (E2's unmeasured ratio leg correctly handled; 2 non-blocking
residuals).

- E1 ✓ threshold "any stall-burst = finding", artifact histogram JSON + burst
  list, canary pinned. E2 ✓ — **the ratio-leg question resolves correctly**:
  ratio gates on the binary leg (measured N=10: 2300 ms median), ours
  process-spawn leg recorded as pending with committed recipe
  (`tools/parity/spec-p3/`), R7 explicitly records "no comparison verdict
  emitted yet", and the E2 canary (DESIGNED, +250 ms injection) is binding
  "before the E2 ratio verdict". No path exists to claim a ratio verdict
  without the missing leg → RATIFY-non-blocking is the correct status.
- E3 ✓ max(2×ref, ref+20 MB/h), CSV+slope, canary-gated. E4 ✓ first-paint p95
  ×1.5, N=20, generator artifact. E5 ✓ 4.5:1/3:1 + ≥ref per cell, walk +
  independence artifacts, canary. E7 ✓ placeholder "any hit = finding" +
  exit-code completeness. E10 ✓ max(2×ref, ref+50 MB/h) + unbounded-monotonic
  rule. E11 ✓ CPU ≤ ref+0.5%, wakeups ×1.5, byte-rate=0 dispositioned. E12 ✓
  ordered result-list equality, committed corpus, per-query diff table.
- **Residual R-A (non-blocking): E6 has an artifact but no assert/threshold** —
  "[M] format-diff table ours vs ref per corpus item" lacks the parity verb
  E12 got ("result-list equality per query"). Diff rows remain visible in the
  artifact (I11 gap-register makes non-reporting an integrity failure, not an
  instrument hole) — below the doc's own G10/G11 standard, not a coverage hole.
  Proposed edit: "…; assert = format-string equality per corpus item; any diff
  dispositioned."
- **Residual R-B (non-blocking): E9 threshold/artifact implicit** — "post-exit
  system-state audit" over enumerated residue classes; spirit threshold
  (zero-orphan children/PTYs/temp-files/locks + terminal restored) unstated.
  Proposed edit: name the audit table artifact + "zero un-dispositioned residue
  = pass".
- E8: no explicit pass criterion but self-canary-ing (planted redaction
  secrets) and I8-gated before first evidence (NONE-DUE, tier rule intact) —
  acceptable.

---

## S3 — environment (B6-B9 riders complete for every cell?)

**Verdict: CLEAN.**

- **B6**: all 8 rows dispositioned (LIVE tmux×{1,2}/no-TERM/16-color/ssh; DOCKER
  screen + xterm-under-Xvfb; DEFERRED-WITH-RECIPE kitty + ghostty/wezterm with
  named headless-render capability condition); two-tier threshold (signature =
  response-byte equality; behavior = frame parity, I6 class-level for animated
  cells); artifact = per-cell table with disposition column. Complete.
- **B7**: every [G] env cell appears in the LIVE list (unset TERM, no HOME, no
  PATH, no git/rg, read-only HOME, LANG×3, TZ absent, OPENCODE_* incl.
  CLI_CONFIG_CONTENT, SSH_TTY, CI vars, docker); ConPTY + macOS deferred with
  recipes; per-OS cells ride OS disposition (stated). Note (cosmetic): linux as
  the live host OS is implicit, never named LIVE.
- **B8**: tool (corpus runner comparing cursor-cell position), corpus (checked
  in, extends A3), threshold (zero desync per class), canary-before-evidence
  with tier order explicitly intact, artifact (per-class cursor table).
  Complete.
- **B9**: all 6 transports dispositioned (LIVE ssh/tmux-over-ssh/container/CI/
  dumb-tty + tc shaping; mosh DOCKER/RECIPE); thresholds per column type
  (echo p50/p95 matrix, redraw ≤ ref×2, boot-succeeds, failure-or-degrade
  assert for ro-HOME/no-/dev/pts); artifact = cell table + latency CSV. Note
  (cosmetic): TERM-negotiation and missing-binaries columns have no dedicated
  threshold — they ride "boot-succeeds + echo matrix".

---

## S4 — fidelity (instrument-layer binding; any clause still inventory-grade?)

**Verdict: CLEAN** (header binds every clause by construction; 2 residuals).

- The I-layer header states it binds EVERY clause, so clauses without an
  explicit I-citation still execute under I1/I5/I9/I11. Verified no clause
  CONTRADICTS the instrument layer (A2's rider corpus vs I5's golden_tests
  binding is complementary, not ad-hoc-rebuilding).
- **Residual R-C (non-blocking): C7 "Background jobs surface" is the thinnest
  clause in the document** — "[F] long-running real job [M] async replay +
  observe": no named artifact, no threshold, "observe" as the only verb. Not
  inventory-grade (real job + replay are behavior), but below the R7 rider
  standard (G3-G6/G10/G11 class). Proposed edit: artifact = per-job-state
  surface table + progress/lifetime assert.
- **Residual R-D (non-blocking): B3** (focus/layering/modality) has no named
  artifact/threshold either ("stack enumeration + key/pointer routing per
  stack"). Independently re-surfaced by the S5 random sample. Proposed edit:
  artifact = per-stack routed-input trace table, assert = routing matches ref
  per legal stack.
- No clause is satisfiable by pure enumeration: the closest (C7, B3) still
  demand behavioral traces.

---

## S5 — granularity (10 fresh random clauses, seed 11, letter-vs-spirit)

**Verdict: CLEAN.**

Sample D6, E9, D8, F7, E3, F1, B3, B2, F3, D9:

- **D6** PASS — served-surface e2e + proxy capture + request-matrix assert;
  desktop/app enumeration-or-absent-justified. Note: app's gui-extensions
  surfaces (806 files) ride the package-level disposition + F2 family
  accounting; transitivity implicit (ties to MG-1's class of implicit rides,
  but D6's letter does force the app disposition).
- **E9** PASS-with-residual — reinforces R-B (threshold implicit).
- **D8** PASS — round-trip diff + output assert (verb present).
- **F7** PASS on its own terms, with the structural observation that the
  ours-minus-ref direction cannot see ref-only families — this is the exact
  mechanism by which MG-1 stayed invisible; the cure is MG-1's (c), not an F7
  direction change (a symmetric diff would drown in ref-dev noise).
- **E3** PASS — threshold + sampling + canary exemplary.
- **F1** PASS, with observation (non-blocking): the coverage ledger has no
  stated aggregate closure condition (no "matrix ≥ X% for LOCK"); acceptable —
  the program doc governs round structure — but R8 may wish to pin one.
- **B3** PASS-with-residual — reinforces R-D.
- **B2** PASS — lifetime-phase + rendered-content clipboard round-trip is
  letter-faithful to the wide-char reconstruction spirit.
- **F3** PASS — concrete scenario set + interleaving assert; artifact unnamed
  (cosmetic).
- **D9** PASS — folding rule (≥2 providers/class) + config→catalog→dialog→
  request chain assert executable and bounded.

No new material gap from S5.

---

## META — whole-document internal consistency

**Verdict: MATERIAL-GAP (MG-2).**

Verified CLEAN (evidence-pinned):

- **Appendix arithmetic**: R1 52 rows (14+13+7+7+11) · R2 30 (8+5+5+6+2+4) ·
  R3 20 (U1-12 + FOLD-1..8) · R5 12 (Q1-Q12) · R7 15 (G1-G15) = **129/129**;
  round-log chain 102 → 114 → 129 consistent.
- **R7 bidirectionality**: G1→I2b ✓ G2→I2b+E2 ✓ G3→B6 ✓ G4→B7 ✓ G5→B8 ✓
  G6→B9 ✓ G7→I6 ✓ G8→I5 ✓ G9→I8 ✓ G10→A2 ✓ G11→B4 ✓ G12→C6 ✓ G13→D3 ✓
  G14→D11 ✓ G15→E2 ✓ — clause {R7: …} tags match destinations both ways.
- **`(a)`-prefixed R2 IDs** in clause tags are R2 part-(a) adequacy re-verdicts
  of R1 gaps (docs/spec-round2.md:33 "## (a) Adequacy verdict per integrated
  R1 gap"), not unledgered new IDs — no loss.
- **Cross-references**: E5(b)→B2∪C16 ✓; E7→C5/D2/C18 ✓; I2b→A5/C6/D3 ✓;
  E6↔B7 boundary ✓ (no boot-cell duplication); B6 rider↔I6 masking rule ✓;
  E2 legs↔I2b source-vs-binary discipline ✓; E5's auditA-deletion consistent
  with B2∪C16 enumeration; A8's merman/latex in-universe ✓.
- **I8 matrix coherence**: DESIGNED-vs-OVERDUE semantics defended in
  docs/spec-round7.md:188-190 ("no axis evidence recorded yet" = no parity
  verdict emitted; E2/E12 OVERDUE-cleared-on-design precedents explicit) —
  coherent under the evidence=verdict reading.
- **I5 instruments**: every named artifact exists in-tree (verified).

### MG-2 — I10b's enumeration-universe pin omits `plugin-browser`

`ls /opt/opencode-ref/packages/` = **35** directories; I10b claims "35 packages
… verified 2026-10-17" but enumerates 6 exhaustive + 28 family-granular =
**34**. The missing entry is **`plugin-browser`** (verified by sorted diff).
It is behaviorally owned by D10 (browser tunnel/proxy/connection/rpc/files),
so no FAMILY is unowned — but the completeness backstop itself is defective:
F2 executes "every export in the **I10b-pinned package universe**" and I10b
requires the round report to list (a)/(b) packages — an agent materializing the
universe from the pin LIST (not the filesystem) never accounts plugin-browser's
exports. In a document whose appendix discipline is "zero-loss", the pin that
everything bottom-up leans on is off by one. One-word fix; still material —
ratification exists precisely to catch this class. **Proposed cure (NOT
applied):** add `plugin-browser` to the family-granular list (or note it as
exhaustive-pinned alongside codemode under D10's ownership).

---

## Register

| ID | Finding | Stance | Blocking? |
|---|---|---|---|
| MG-1 | Telemetry/observability family unowned; ref wires OTLP Observability at CLI boot (`cli/src/index.ts:126`); F7 R5-Q12 disposition carries false "no implementation" premise; ours has zero OTLP — live divergence invisible | S1 | **MATERIAL** |
| MG-2 | I10b pin claims 35 packages, enumerates 34; `plugin-browser` (D10-owned) missing from F2/I7 completeness backstop | META | **MATERIAL** |
| R-A | E6 lacks assert/threshold (format-diff table only) | S2 | no |
| R-B | E9 threshold/artifact implicit | S2/S5 | no |
| R-C | C7 thinnest clause — no artifact/threshold, "observe" verb | S4 | no |
| R-D | B3 no named artifact/threshold | S4/S5 | no |
| N-1 | B7 linux disposition implicit; B9 TERM-negotiation/missing-binaries thresholds ride boot+echo | S3 | no |
| N-2 | F7 ours-minus-ref direction structurally blind to ref-only families (mechanism of MG-1; cure via MG-1(c)) | S5 | no |
| N-3 | F1 has no aggregate coverage closure condition | S5 | no |
| N-4 | gui-extensions (806 files) owned transitively via D6/U10 + F2 — transitivity implicit | S1 | no |

## Overall call

**RATIFY-1: FAIL.** Two material gaps (MG-1, MG-2). Per docs/scan-program.md:
not a ratification — route to **R8 integration** (cures proposed above, none
applied by this lane), convergence counter resets, then a new FINAL candidate
before RATIFY-1′/RATIFY-2. The 129/129 appendix, riders, canary matrix, and
instrument layer are otherwise coherent and verified against the trees; the
failures are narrow and precisely curable.
