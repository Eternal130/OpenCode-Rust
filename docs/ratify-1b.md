# RATIFY-1′ — Fresh first ratification round on FINAL candidate 2 (per docs/scan-program.md)

- **Under review**: docs/scan-spec.md **v7** (1048 lines; appendix 140/140; SPEC-R8 cures
  for RATIFY-1's MG-1/MG-2 declared in docs/spec-round8.md; FINAL candidate 2).
- **Lane**: `lane/ratify1b` (worktree `.worktrees/lane-ratify1b`). Stances S1-S5 + META,
  one full pass each, fresh eyes on v7, PLUS a regression check on the two R8 cures.
  No clause edits applied (ratification judges; the one material finding below is
  proposed for R9).
- **Verdict summary**: S1 MATERIAL-GAP · S2 CLEAN · S3 CLEAN · S4 CLEAN · S5 CLEAN ·
  META CLEAN (3 cosmetic residuals).
  **Overall: RATIFY-1′ FAIL** → R9 integration, convergence counter resets again.
  One material gap: **MG-3 A8's mermaid sub-family enumeration lists 4 of ref's 6
  diagram kinds (`state` and `timeline` missing)** — the MG-2 defect class surviving
  inside a clause, in the round that introduced census discipline.
  Both R8 cures themselves verified SOUND (no new false premise introduced).

---

## Method + environment facts (measured this round, 2026-10-17)

- Ref tree `/opt/opencode-ref/packages/` = **35 packages**; **census command
  reproduces the pin exactly** (`ls -1 … | sort` sorted-diff vs the I10b pin list =
  empty; `wc -l` = 35; 6 exhaustive + 29 family-granular arithmetic checks out).
  Ref binary `/root/.opencode/bin/opencode` runs (2.0.25).
- All I5-bound instruments exist in-tree (re-verified file-by-file, incl.
  `tools/parity/spec-p3/` and ref `packages/simulation`).
- S1 package selection by fresh mention-census (spec + round docs + findings),
  EXCLUDING RATIFY-1's set {console, storybook, containers, httpapi-codegen,
  effect-drizzle-sqlite, http-recorder, util, gui-extensions}: mission-suggested
  {ai, plugin, schema, merman, latex, posts} + census-minimum non-overlaps
  {web (5 mentions), function (7)}.
- S5 sample: `random.Random(23).sample` over the **83-clause universe** (v6 was 82;
  C22 added) in document order → **C7, I10, I2b, E12, C9, D2, C18, E4, C15, A4**
  (reproducible; zero overlap with RATIFY-1's seed-11 sample).

---

## S1 — feature-space (8 least-deep-dived packages, fresh set)

**Verdict: MATERIAL-GAP (MG-3).**

| Package | Reality (verified this round) | Owned? |
|---|---|---|
| ai | AI-provider SDK: 60+ provider files (chat, speech: assemblyai/deepgram/cartesia/elevenlabs; image/video: fal/replicate/runway/stability/black-forest-labs; opencode-zen…); consumers = core + app | ✓ D9's folded matrix (the ≥2-providers-per-class rule exists for exactly this) + capability flags (supportsText/image); F2 family-granular |
| plugin (SDK pkg) | 63-file plugin API package (effect/{skill,storage,session,vcs,command}); consumers = cli + core | ✓ exhaustive pin; I2/A5/C6/D3 family (240 mentions) |
| schema | 117 schema modules (session, permission-saved, skill, instruction…); consumers = ai/app/core | ✓ exhaustive pin; shapes flow to D1/D2/C20/D4; A3 materializes part-kinds from our oc-schema |
| merman | TUI mermaid renderer; **6 diagram kinds** — see MG-3 | ⚠️ A8 (enumeration defective) |
| latex | 14-file TeX layout renderer (parser/layout/render/symbols) | ✓ A8 (inline vs block named; ours absent-justified) |
| posts | 3 ts files + astro config = static site | ✓ I10b standing entry accurate |
| web | share viewer (`Share.tsx` polls `ws /share_poll?id=`) + docs site | ◐ D8 destination-cell transitivity implicit — residual R-E |
| function | Cloudflare worker: share backend (`/share_create` `/share_sync` `/share_poll` `/share_delete[_admin]`, SyncServer DurableObject, R2, GitHub-App auth) | ◐ F2 generic accounting only, no standing entry — residual R-E |

### R-E (residual): the hosted share pair `function`+`web` has no standing disposition entry

Verified: **no product-side client exists** in the pinned 2.0.24 source (only
`function` defines the routes; `web` polls) NOR in the 2.0.25 binary (`--help` has
no share command; 0 `share_create` strings). The `share` config field exists on
both sides (ref `normalize.ts:99/:209`, ours `normalize.rs:295/:499` — C4's
per-leaf-key letter covers it). So this is the `stats`-class (hosted server-side,
no client in the product under test) — but unlike stats/identity/enterprise/
console/posts/script, `function` and `web` have **no I10b standing OOC/accounting
entry**; every F2 round must re-adjudicate them from scratch. D8 owns the share
product family ("publish/unpublish lifecycle … format x destination") and I10's
re-baseline-on-release rule will catch a future product client. Non-blocking:
no false premise exists anywhere in the spec about these packages; the census
pins them; F2's letter ("every export … accounted") reaches them. Proposed cure
(for R9, NOT applied): two standing accounting entries (function = hosted share
backend, no client in pinned ref source/binary; web = share viewer + docs site,
viewer rides D8 destination cells).

### MG-3 — A8's mermaid sub-family enumeration is 4 of 6: `state` and `timeline` are missing

Evidence (all fresh, this round):

1. **Ref's own type union is the ground truth**: `merman/src/diagnostics.ts:1` —
   `MermaidDiagramKind = "flowchart" | "sequence" | "state" | "timeline" |
   "gitGraph" | "gantt"` — **six kinds**; `detect.ts` dispatches exactly these six.
2. **`state` and `timeline` are full renderers**, not stubs: `src/state/` (16 files:
   parser/diagram/layout/routing/drawing/render-grid/style/visible-model + tests),
   `src/timeline/` (6 files: parser/diagram/drawing/render-grid/style + tests) —
   same file shape as the four listed kinds.
3. **A8 (scan-spec.md:201-202) enumerates only "flowchart/gantt/gitgraph/sequence"**.
   The list originates in spec-round2.md:126 (R2's prose inventory) and was carried
   verbatim into A8 at R3; spec-round3.md:73 re-quoted the same four when naming
   sub-families. **No round ever censused the kind union.**
4. Not covered elsewhere: grep for timeline/state-diagram across spec + round docs
   hits only *unrelated* timelines (`session.timeline` dialog, session-ui timeline
   renderers — different family). The mermaid `timeline`/`state` kinds are visible
   to no clause.

**Materiality** (calibrated honestly): today's verdict impact is **zero** — ours
ships no mermaid renderer at all, so every merman sub-family cell holds the same
absent-justified disposition regardless of count; no false product premise is
carried anywhere (unlike RATIFY-1's MG-1). But this fails the program's **own
standard set at MG-2 and re-legislated in v7's census discipline** ("the pin is
mechanical, not prose … never against recall"): A8's sub-family list is the [G]
enumeration its corpus generation leans on, it is contradicted by the ref source's
own union, and the I11 verbatim-carry discipline has propagated it R2→v7
untouched. An agent materializing A8's corpus from the letter omits two entire
diagram kinds (a fortiori if ours ever adds mermaid). Ratification exists
precisely to catch this class. **Proposed cure (for R9; NOT applied):** rewrite
A8's parenthetical to "sub-families = the `MermaidDiagramKind` union
(`merman/src/diagnostics.ts`): flowchart/sequence/state/timeline/gitGraph/gantt"
— extending census discipline from packages (I10b) to load-bearing in-clause
enumerations.

---

## S2 — quality (E-clause executability)

**Verdict: CLEAN.**

- All 12 E-clauses carry artifact + threshold (or are explicitly self-canary-ing
  and I8-gated): E1 stall-burst ✓, E3 max(2×ref, ref+20 MB/h) ✓, E4 p95 ×1.5
  N=20 ✓, E5 4.5:1/3:1 + ≥ref per cell ✓, E6 format-string equality per corpus
  item ✓ (**R8-N1 rider landed**, scan-spec.md:680), E7 any-placeholder-hit +
  exit-code completeness ✓, E9 residue audit table + zero-un-dispositioned ✓
  (**R8-N2 rider landed**, :702), E10 max(2×ref, ref+50 MB/h) + monotonic rule ✓,
  E11 CPU/wakeup/byte-rate ✓, E12 ordered result-list equality ✓. E8 acceptable
  per RATIFY-1 precedent (self-canary-ing, NONE-DUE tier intact).
- **E2 ratio-leg canary-binding status: CORRECT.** The ×1.5 ratio gates on the
  binary leg (measured N=10: 2300 ms median); ours process-spawn leg recorded
  pending with committed recipe (`tools/parity/spec-p3/` exists); the E2 canary
  (DESIGNED, +250 ms injection) is binding "before the E2 ratio verdict"; **no
  ratio verdict is claimed anywhere in v7** (E2 text and round log both carry
  only the ref-leg measurements, consistent at 5250/2300 ms). No path to a ratio
  verdict without the missing leg.
- Housekeeping note: the mission brief's "E6/E9 riders (R8-N2/N3)" label is off
  by one — in the spec E6 = R8-N1 (=R-A), E9 = R8-N2 (=R-B), C7 = R8-N3, B3 =
  R8-N4. All four riders are landed and tagged; the appendix rows carry the
  (=R-A…R-D) equivalences explicitly, so bidirectionality holds.

---

## S3 — environment (B6-B9 + the new B7 OTEL cells)

**Verdict: CLEAN.**

- **B6** complete (8 rows dispositioned: LIVE tmux×{1,2}/no-TERM/16-color/ssh;
  DOCKER screen + xterm-under-Xvfb; DEFERRED-WITH-RECIPE kitty/ghostty-wezterm;
  two-tier threshold; cell-table artifact). Unchanged from v6, re-checked.
- **B7** complete; the **R8 OTEL_* cells (:292-300) are complete against C22's
  env-read set**: {OTEL_EXPORTER_OTLP_ENDPOINT set; OTEL_EXPORTER_OTLP_HEADERS
  valid+malformed; OTEL_RESOURCE_ATTRIBUTES valid+malformed} — all three vars
  covered, malformed variants exactly where ref parses defensively (verified:
  `parseHeaders` never throws; `resourceAttributes()` catches and drops bad
  attrs — `otlp.ts:29-43`). LIVE as plain `env(1)` cells is right. The
  **E6↔B7-style boundary is stated and correct** (B7 = boot/survival/smoke;
  C22 = observability behavior), including the ours-today case (no OTEL_* reads
  to survive; the cell still runs so boot must not regress when vars present).
  Cosmetic only: the v6 LIVE list at :284-287 does not repeat OTEL_* (the R8
  rider block extends the set; no contradiction).
- **B8** complete (corpus runner, cursor-cell criterion, zero-desync threshold,
  canary-before-evidence with tier order intact). **B9** complete (6 transports
  dispositioned; per-column thresholds; failure-or-degrade assert for
  ro-HOME/no-/dev/pts). Unchanged from v6, re-checked.

---

## S4 — fidelity (instrument-layer binding; C22's method real?)

**Verdict: CLEAN** (1 cosmetic residual).

- **C22's method is real and executable as written.** Every ref-side factual
  claim was verified against source, line-exact:
  `cli/src/index.ts:8` (import) and `:126` (`Observability.layer({endpoint:
  process.env.OTEL_EXPORTER_OTLP_ENDPOINT, headers: …, client/version/channel})`)
  — both exact; env-read set is exactly the 3 named vars
  (`observability.ts:22-23`, `otlp.ts:30`); layer provided unconditionally with
  emission endpoint-gated (`otlp.ts:64,:75`) and `Layer.catchCause(() => local)`
  fallback (`observability.ts:51`); OTLP JSON via `OtlpSerialization.layerJson` +
  `FetchHttpClient.layer` + `OtlpExporter.layerFlusher` (`:43-45`); logs →
  `{endpoint}/v1/logs`, traces → `{endpoint}/v1/traces` with BatchSpanProcessor
  and AsyncLocalStorage-ContextManager set as GLOBAL (`otlp.ts:89-97`); headers
  parsed `k=v,k=v` (`parseHeaders`); resource = serviceName `opencode` +
  serviceVersion + `deployment.environment.name`=channel + `opencode.client` +
  `opencode.run`/`service.instance.id` = runID = `crypto.randomUUID().slice(0,8)`
  per process (`shared.ts`); config key at `core/src/config/normalize.ts:49`
  exact; file-log layer constants exact (`logging.ts:9-13`: 50 MB cap → 25 MB
  keep, "1 hour" trim, 5-min stale lock), structured `key=value` lines with
  timestamp/level/run/spans/annotations (`formatter`, :16-31), machine-wide
  append-only shared file, `Logging.loggers(local, channel)` naming (`:175`,
  `file()` :59-62) — and the file layer is genuinely always-on in both the
  endpoint-set and endpoint-unset branches plus the fallback.
  The mock-collector assert is executable as written: plain loopback HTTP
  recorder + `env(1)` on the ref binary; the traffic-gating canary pair
  (set ⇒ /v1/logs + /v1/traces received; unset ⇒ zero traffic, file log still
  writes) is well-defined; ours-side disposition (iv) blocks vacuous parity.
- **Ours-side claims true**: `grep OTEL_ crates/` = 0, `grep otlp crates/` = 0
  (the 3 `openTelemetry` hits in `oc-core/config/normalize.rs` are the C4 config
  key leaf — mirroring ref's own `normalize.ts` key recognition — not OTel
  implementation; consistent with C22(4)/F2's wording).
- Every clause remains instrument-bound via the I-layer header (re-verified all
  I5-named instruments exist in-tree, including `spec-p3/` and ref
  `packages/simulation`).
- **Residual R-F (cosmetic):** C22's mock OTLP collector is declared C22-local
  ("the standing assert target") but is not yet in I5's named-instrument list;
  until first execution builds it, its standing-ness rests on C22's own text.
  R9 may add it to I5 alongside the first artifact.

---

## S5 — granularity (10 fresh random clauses, seed 23, letter-vs-spirit)

**Verdict: CLEAN.** Sample C7, I10, I2b, E12, C9, D2, C18, E4, C15, A4:

- **C7** PASS — the R8-N3 rider cured RATIFY-1's R-C: artifact (per-job-state
  surface table) + assert (progress advances; start/update/end lifetime events
  on the owning surface, ours-vs-ref columns). Cosmetic: the job-state
  enumeration source is unnamed (C1's "from ref code, not prose" discipline
  would suit it).
- **I10** PASS — pin/re-baseline/record-with-report; the 2.0.24-tree +
  2.0.25-binary pins are recorded per I2b and honored across E2/C22.
- **I2b** PASS — precise, evidence-pinned (RESCAN F10); the binary runs; the
  record-which-ref-form rule is stated for both directions.
- **E12** PASS — committed corpus (exists in-tree), ordered result-list
  equality, rank-order fuzzy parity with tie disposition, normalization audit,
  per-query diff artifact; corpus-poisoning canary bound before second pass.
- **C9** PASS — {add, connect, auth, fail, restart, remove} × surfaces ×
  tool-invocation/needs-auth/elicitation-blocked; live round-trip trace with a
  real local MCP server is executable. Cosmetic: threshold implicit in the
  round-trip verb.
- **D2** PASS — endpoint × consumer × happy × error codes × malformed +
  request-side field parity via proxy capture; reuses D6's capture.
- **C18** PASS — enumeration source named (`cli/src/commands/handlers/*`,
  which exists and is materializable); executed-command matrix with every cell
  dispositioned; headless stream format + exit codes included.
- **E4** PASS — generator-corpus (I9-labeled synthetic), first-frame marker,
  N=20, ×1.5, clean A3 boundary (E4 owns time only).
- **C15** PASS — triggers + payload-resolution cells + frecency/history/stash
  in [G]; the wrong-payload-with-right-chip catch is the spirit, well encoded.
- **A4** PASS — per-frame content at every animated element × phase under I3;
  cosmetic: ref-side clock-stepping is implicit (ours steps the vt-clock; ref
  is captured at pinned real cadences).

No material gap from S5.

---

## META — whole-document internal consistency

**Verdict: CLEAN** (3 cosmetic residuals; counts re-measured, not trusted).

- **Appendix arithmetic exact (row-counted this round)**: R1 52 · R2 30 · R3 20
  (12+8) · R5 12 · R7 15 · R8 11 = **140/140**; header claim (:27-28) and legend
  match; round-log chain 102 → 114 → 129 → 140 consistent.
- **Bidirectionality**: R8 rows ↔ destinations verified both ways — MG1 → C22 +
  B7 cells + F2 entry + I8 addendum (all tagged {R8: MG-1}); MG1-F7 → F7 rewrite
  (:791-798); MG2 → I10b correction (:130-142); N1-N4 → E6/E9/C7/B3 riders with
  explicit (=R-A…R-D) equivalence tags; N5/N7/N8 → REG. U2 and R5-Q12 rows carry
  the R8 updates; U8 row correctly splits server-side stats (OOC) from client
  observability (→ C22).
- **REG-row semantics**: N5/N7/N8 correctly registered (non-blocking, no clause
  change — legend honored). **Cosmetic M-1: R8-N6's status letter "A" is a
  mislabel** — its row records a structural cure via C22 + a decision (F7
  direction unchanged), not an amendment folded into an existing clause; REG or
  a C22 reference would be the honest letter. Zero-loss property unaffected.
- **Cross-references all resolve**: E5(b)→B2∪C16; E7→C5/D2/C18; I2b→A5/C6/D3;
  E6↔B7 and C22↔B7 boundaries (no duplicated boot cells); C22→C4 (config-key
  leaf), →F7 (env-read scope), →I8 (canary addendum), →F2 (accounting), →U8
  (server/client split); F2/I7→I10b census. Clause numbering sequential
  (A1-9, B1-9, C1-22, D1-11, E1-12, F1-7, I1-I11 incl. I2b/I10b).
- **v7's growth damaged nothing**: v6 texts riders reference are intact (B6/B7/
  B8/B9 riders, E2 legs, A2/B4/C6/D3/D11 rewrites); measured E2 numbers
  consistent between clause and round log; lineage + round-log v7 entry
  consistent with the header.

---

## PLUS — regression check on the two R8 cures

**Both cures sound; no stale claim survives; nothing new introduced.**

- **MG-1 (telemetry)**: every "no implementation" occurrence in v7 (:793, :852,
  :1035) is the corrective narrative quoting the dead R5 wording — no live
  stale premise. C22's "R5 examined core only" is accurate (spec-round5.md:221).
  C22's I2b phrasing survives scrutiny: "telemetry lives in the CLI boot path,
  NOT in the 2.0.24-source `packages/core`" — precise (it lives in cli/util of
  the same tree; core-only scans missed it; the binary leg is the executable
  ground truth). F2's entry correctly marks the family "NOT absent-justified …
  a live divergence held in C22's ledger". B7's rider handles the ours-today
  case correctly. No other clause contradicts the cure (grepped OTEL_/otlp/
  observability/openTelemetry across the full spec: 19 OTEL_ hits, all in cure
  contexts).
- **MG-2 (census)**: the mechanical census command reproduces the pin exactly
  (sorted diff empty; 6+29 arithmetic true); no second copy of the old 34-list
  exists anywhere; D10/U2 cross-links updated; "verified 2026-10-17, RATIFY-1
  and this round" re-verified a third time by this lane.
- **New-premise scan of the cure text itself**: C22/B7/F2/I8 cure wording
  verified against ref source line-by-line (S4 above) — no overclaim found. The
  one enumeration-precision defect found this round (MG-3) is **pre-existing**
  (R2→v7), not introduced by R8.

---

## Register

| ID | Finding | Stance | Blocking? |
|---|---|---|---|
| MG-3 | A8 mermaid sub-family enumeration lists 4 of ref's 6 kinds — `state` + `timeline` missing; ref ground truth = `MermaidDiagramKind` union (`merman/src/diagnostics.ts:1`, `detect.ts` dispatches 6; both are full renderers); list carried R2→v7 verbatim, never censused; zero current-verdict impact (renderer wholly absent-justified) but fails the program's own census discipline (MG-2 class) | S1 | **MATERIAL** |
| R-E | Hosted share pair `function` (CF worker backend) + `web` (viewer/docs) has no I10b standing entry; no product client in pinned source or 2.0.25 binary (verified); F2 generic accounting + D8 + I10 re-baseline cover it | S1 | no |
| R-F | C22's mock OTLP collector is clause-declared standing, not yet I5-named | S4 | no |
| M-1 | R8-N6 appendix status letter "A" mislabels a structural cure (via C22) as an amendment | META | no |
| M-2 | B7 v6 LIVE list doesn't repeat OTEL_* (the R8 rider block extends it; no contradiction) | S3 | no |
| M-3 | C7 job-state enumeration source unnamed; C9 threshold implicit; A4 ref-side clocking implicit | S5 | no |

## Overall call

**RATIFY-1′: FAIL.** One material gap (MG-3). Per docs/scan-program.md: route to
**R9 integration** (one-line enumeration cure + optional census-discipline
extension to in-clause enumerations; R-E/R-F/M-1..M3 riders optional), counter
resets, then a new FINAL candidate before the next ratification round.

Honest calibration for R9: MG-3 is materially narrower than either RATIFY-1 gap
— no live divergence, no false product premise, no unowned family (A8 owns the
mermaid family; the whole renderer is absent-justified today). It fails on the
document's own enumeration-exactness standard, which v7 itself sharpened into
"census discipline: mechanical, not prose". The verdict follows the standard the
program set for itself; if the program prefers to re-legislate that standard for
in-clause [G] enumerations (as it did for RATIFY-non-blocking residuals in S2),
that is an R9/program-owner decision — but as written, the letter of A8 is
contradicted by the ref source, and ratification exists to say so.

The two R8 cures are otherwise exemplary: C22's every factual claim verified
line-exact against ref source; the census pin is now mechanically reproducible;
no regression and no new false premise was introduced by the cures.
