# RATIFY-1‴ — Fresh first ratification round on FINAL candidate 4 (per docs/scan-program.md)

- **Under review**: docs/scan-spec.md **v9** (1385 lines; appendix 162/162; SPEC-R10 cures
  for RATIFY-1″'s MG-4 declared in docs/spec-round10.md; FINAL candidate 4).
- **Lane**: `lane/ratify1d` (worktree `.worktrees/lane-ratify1d`). Stances S1-S5 + META,
  one full pass each, fresh eyes on v9, PLUS a regression grep over every cured class
  (the four prior MG/M classes: telemetry family, A6 count, mermaid lists, C14 prompt
  source). No clause edits applied (ratification judges).
- **Verdict summary**: S1 CLEAN · S2 CLEAN · S3 CLEAN · S4 CLEAN · S5 CLEAN ·
  META CLEAN (4 non-blocking nanos).
  **Overall: RATIFY-1‴ PASS** → proceed to **RATIFY-2‴** per docs/scan-program.md.
  The strengthened enumeration-census guard HELD under full independent re-execution:
  the R10 sweep's discovery procedure re-runs mechanically and reproduces the 83-clause
  universe exactly; its §3.1 table covers all 83 rows; every census pin, cure, and count
  annotation re-executed line-exact against the ref tree this round. Zero material gaps.

---

## Method + environment facts (measured this round, 2026-10-18)

- Ref tree `/opt/opencode-ref/packages/` = **35 packages** (`ls -1 | sort | wc -l` = 35,
  re-executed). Ref binary `/root/.opencode/bin/opencode` (2.0.25) per I2b (not needed
  this round — no plugin-era cell was judged). Ours-side greps against
  `/opt/opencode-rust/crates/`.
- **Discovery procedure re-run (the META core, per R10 §3.0)**:
  `grep -oE '^(I[0-9]+b?|[A-F][0-9]+) (\*\*|[A-Za-z])' docs/scan-spec.md | wc -l`
  → **83**, and the extracted ID sequence is EXACTLY the true universe (I1..I11 incl.
  I2b/I10b · A1-9 · B1-9 · C1-22 · D1-11 · E1-12 · F1-7) — no line-wrap stray, no E6
  drop (the `[A-Za-z]` class does its job). R10 §3.1's table rows 1-83 cover exactly
  these 83 IDs — **no clause is missed by the procedure** (the mission's material-gap
  bar).
- **Every v9-touched census re-executed independently, all exact**: I10b 35/35 + the
  five R-G1 `find <pkg> -type f` counts = 8/31/2/16/30 + identity = 6; A5 slots =
  exactly {home, prompt, sidebar, system}; A6 grep = 17 files = 13 panels + 4 plumbing
  (numeral now coherent); A8 all claims (kind union 6 at `diagnostics.ts:1`; per-kind
  `find` counts 12/10/18/7/7/7 — note the gitGraph DIRECTORY is lowercase `gitgraph/`,
  7 files, matches the pin's kind count; core = 15 via `find` because `color/` is a dir
  of 2; markdown/palette/plugin/layout-audit all exist; latex `plugin.ts:11-12` two
  registrations; packages-wide `registerCodeBlockRenderer` = 3 call sites + 1 API
  declaration = the pin's "two builtin registrations (merman; latex 'latex'+'math')";
  ours `grep -ri 'mermaid\|merman\|latex\|katex' crates/` = **0**); C3 `permission.ts:41`
  Reply {once,always,reject} + `:55` Effect {allow,deny,ask}; C7 `session-message.ts:
  127/:133/:140/:148` (extras at :249/:258 are the Compaction union, correctly outside);
  C8 `Effect.fn("Worktree.…"` = exactly create :204 / remove :257 / refresh :273 /
  list :330; C11 `updater.ts:14` = the exact 7-method array; C13 `skill.ts:45` =
  `DirectorySource | UrlSource | EmbeddedSource`; C16 = 23 ∪ 4 dirs; C18 = 18 non-test
  handler entries matching the pin's list; C20 tool dirs 6 + 15 (+ AGENTS.md doc note);
  D6 desktop/src = exactly {main, preload, renderer, shared} + every named app/src
  subpackage exists (full listing re-checked); C22 env-read set = exactly the 3 OTEL_*
  vars (`observability.ts:22-23`, `otlp.ts:30`, `cli/index.ts:127-128`) + `cli/index.ts:8`
  import and `:126` `Observability.layer({` line-exact; ours `grep -ri 'OTEL_\|otlp'
  crates/` = **0**; I3 value pins re-verified (spinner 80 `spinner.tsx:21`, backoff
  1000/5000 `app.tsx:1313`, StartupLoading 500 `:46` / 3000 `:22`).
- **C14 cure re-executed 100%** (see PLUS below): all seven census claims of the v9
  cure block reproduce line-exact.
- **Appendix arithmetic row-counted**: R1 52 · R2 30 · R3 20 · R5 12 · R7 15 · R8 11 ·
  R9 17 · R10 5 = **162/162**; legend, header claim, and round-log chain
  (114 → 129 → 140 → 157 → 162) all consistent.
- **S5 sample**: `random.Random(37).sample` over the 83-clause universe in document
  order → **F2, I10b, F4, E2, I4, C17, D4, E4, A1, D6** (reproducible; my ordering
  validated by reproducing ratify-1c's seed-29 sample exactly). Honest note: F2, I10b,
  E2 also appeared in RATIFY-1″'s seed-29 sample — all three were re-judged fresh
  against v9 text, and all three sit in v9's actual change-set (I10b guard + entries;
  F2's standing-entry ledger extended by R-G1).
- LIVE environment tools present (tmux, sshd loopback, docker, tc).

---

## S1 — feature-space (8 packages: protocol, plugin-browser, codemode, sdk, stats, enterprise, server, app)

Selection: prior S1 lanes consumed 24 of 35 (RATIFY-1: console, storybook, containers,
httpapi-codegen, effect-drizzle-sqlite, http-recorder, util, gui-extensions; RATIFY-1′:
ai, plugin, schema, merman, latex, posts, web, function; RATIFY-1″: client, containers,
script, storybook, theme, session-ui, identity, effect-drizzle-sqlite). Eleven packages
remained never-deep-dived (app, codemode, desktop, enterprise, plugin-browser, protocol,
sdk, server, simulation, stats, ui — core/tui/cli are the perennially census-covered
exhaustive trio). The 8 with the least coverage across ALL round docs (mention-census:
protocol/plugin-browser/codemode/sdk carry ZERO ratify-lane rows; stats/enterprise only
as OOC labels): **protocol, plugin-browser, codemode, sdk, stats, enterprise, server,
app**.

**Verdict: CLEAN** (2 nanos). Fresh reality-check per package:

| Package | Reality (verified this round) | Owned? |
|---|---|---|
| protocol | 48 files: `api.ts` + 28 `groups/` (agent…worktree) + 2 middleware + client/errors/simulation.ts — the canonical HttpApi definition layer; consumers (grep `@opencode/protocol`): server 30 files, client 5, cli 2, plugin 1, simulation 1 | ✓ transitively: D2's endpoint parity + D1's SSE materialize from it at run time (both adjudicated data-driven/runtime-named — the same class R9/R10 gave D2); F2 letter reaches its exports (in the pinned universe) |
| plugin-browser | 7 src files: connection/files/index/proxy/rpc/tools/tunnel | ✓ **D10** names five of seven verbatim ("browser tunnel/proxy/connection/rpc/files round-trips"); tools/index ride |
| codemode | 101 files: interpreter/, stdlib/, openapi/, tool-runtime.ts, tool-schema.ts, extension.ts, namespace.ts, … | ✓ **D10** [G] names interpreter/stdlib/openapi/tool-runtime/tool-schema — all five axes exact |
| sdk | 33 files: contracts, effect/, promise, internal/, workerd, logging, tool | ✓ **D11** names `packages/sdk` .d.ts shape-diff explicitly |
| stats | 111 files = THREE sub-packages (app = Vite frontend w/ own package.json; core = drizzle + migrations; server = API + Dockerfile) — a self-hosted product-analytics stack, entirely separate from `SessionStats` (session token usage, core-owned, ours ports it at `oc-schema/src/session_stats.rs`) | ✓ U8 OOC entry accurate ("we ship no stats surface"); nano N-b below on the "server-side" wording |
| enterprise | 24 files: SolidStart web app — `routes/share/[shareID].tsx`, `routes/api/[...path].ts`, `core/{share,share-document,storage}.ts`, entry-client/server — i.e. a SELF-HOSTABLE third member of the hosted share family (function = CF backend, web = viewer/docs) | ✓ U9 OOC ("not shipped by us") valid; nano N-a below on the label |
| server | 89 files: api/auth/cors/event-feed/fetch/pairing/process/pty-environment + handlers/ (29 group impls mirroring protocol) + middleware | ✓ transitively: D1 (event-feed), D2 (handlers), C10 (pairing.ts), C17 (pty.ts/pty-socket.ts), I2b detached-server discipline; `request-tracing.ts` inspected = 2-export span-context helper used by process.ts, not a telemetry family (C22 boundary intact) |
| app | 797 files: app.tsx, composer, desktop.ts, home, new-session, providers, runtime, servers, session, settings, shell, workspaces (D6's census pin listing re-executed — exact, the "…" honest) | ✓ **D6 (U10 amendment)**: served-surface enumeration or absent-justified; pin verified |

No unowned family, no false premise about any package's contents. The F2 completeness
backstop now has standing entries for every accounting-class package (R-G1 closed the
asymmetry); the eight above are all either clause-named (D6/D10/D11), transitively
data-driven (D1/D2/C10/C17 for server; D2/D1 for protocol), or OOC-with-justification
(U8 stats, U9 enterprise).

---

## S2 — quality (E-clause executability)

**Verdict: CLEAN.**

- The v8→v9 diff touches NO E-clause (verified by `git diff 564f75d 7732190` — hunks
  land only in header/lineage, integration rule, I10b, A6, C13, C14, C20, round log,
  appendix). All 12 E-clauses re-checked for artifact + threshold markers
  programmatically: every one carries both (E1 stall-burst + histogram JSON; E3
  max(2×ref, ref+20 MB/h); E4 first-paint p95 ×1.5 N=20; E5 4.5:1/3:1 + ≥ref-per-cell;
  E6 R8-N1 format-string equality; E7 placeholder scan + exit-code completeness; E8
  probe set; E9 R8-N2 zero un-dispositioned residue; E10 max(2×ref, ref+50 MB/h) +
  monotonic rule; E11 CPU/wakeup/byte-rate=0; E12 ordered result-list equality).
- E2's two ref legs + binary-gating unchanged; measured numbers (source-dev 5250/5300,
  binary 2300/2400) consistent clause↔round-log; ours leg honestly pending with
  committed recipe (`tools/parity/spec-p3/` exists, incl. `refbin_tti_results.json`).
- The four R8 riders (E6/E9/C7/B3) all present verbatim (grep `R8-N1..N4 rider` = 1
  each); C7's v8 census pin + C9 threshold note + A4 clock note intact (ratify-1b M-3
  stays cured).

---

## S3 — environment (B6-B9 + the OTEL cells)

**Verdict: CLEAN.**

- B6/B7/B8/B9 riders intact (per-cell tool/threshold/disposition structure; LIVE /
  DOCKER / DEFERRED-WITH-RECIPE). LIVE tools re-verified present on host (tmux, sshd,
  docker, tc). B8 canary-before-evidence tier order still coherent (no B8 evidence
  exists yet).
- B7's v6 LIVE list names OTEL_* (the M-2 cure holds); the R8 OTEL_* cell set is
  complete against C22's env-read set — all 3 vars re-verified line-exact this round
  (ENDPOINT `observability.ts:22`/`cli/index.ts:127`; HEADERS `:23`/`:128`;
  RESOURCE_ATTRIBUTES `otlp.ts:30`); malformed-value cells match ref's defensive
  parsing (otlp.ts :36 throws are caught/dropped per entry). The E6↔B7-style
  B7↔C22 boundary (boot/survival vs observability behavior) stated and correct.

---

## S4 — fidelity (instrument layer; I5 binding; C22 evidence base)

**Verdict: CLEAN.**

- All I5-named standing instruments existence-checked in-tree this round:
  `golden_tests.rs`, `frame_budget_tests.rs`, `replay.rs`, `mouse_modes.rs`,
  `diff_viewer_golden_tests.rs` (crates/oc-tui), `spec_e1_canary.rs`,
  `spec_e5_canary.rs`, `spec_e2_tti.rs`, `spec_e12_first_pass.rs` +
  `e12_search_corpus.json`, `e3_leak_canary.rs`, `rescan-tui.rs`
  (tools/parity/src/bin), `tools/parity/spec-p3/` (incl. refbin_tti_results.json), ref
  `packages/simulation` — all present; zero ad-hoc rebuild drift.
- I5's R9 extension (C22 mock OTLP collector named standing, committed at first C22
  execution) unchanged and still honestly un-executed (no collector in-tree — the
  rule predicts exactly this state).
- C22 spot re-verification line-exact: `cli/src/index.ts:8` (Observability import),
  `:126` (`Observability.layer({`); env-read set = exactly 3 vars; config key
  `openTelemetry` in `core/src/config/normalize.ts:49`; ours side greps = 0 hits for
  both OTEL/otlp and mermaid/merman/latex/katex (the absent-justified premises hold
  with margin).
- I8 canary-coverage matrix coherent; C22 design addendum intact; defer-or-design rule
  unbroken (no new evidence legs were opened by v9).

---

## S5 — granularity (10 clauses, seed 37, letter-vs-spirit)

**Verdict: CLEAN — 10/10 PASS.**

- **F2** PASS — export accounting vs the I10b universe; every standing entry
  re-verified against reality (devtools, util/observability→C22, console/posts/script,
  function/web, the five R-G1 entries — all census counts reproduced this round), plus
  this lane's 8 fresh S1 censuses add no unaccounted package.
- **I10b** PASS — the pin + census discipline commands reproduce exactly (35/35); the
  strengthened guard's letter is coherent and, decisively, its exhaustive-sweep rule is
  **re-runnable and reproduces** (META below).
- **F4** PASS — degradation matrix authored-by-class; the one ref-fact set it leans on
  (backoff 1s/5s) is I3-value-pinned and re-verified line-exact (`app.tsx:1313`).
- **E2** PASS — see S2.
- **I4** PASS — one-line fidelity keystone ("token-exact cells per element x state");
  no enumeration carried; materialized by A1/A3/I6's harnesses. Thin by design, not
  defective.
- **C17** PASS — verbs authored; ALL FOUR named surfaces existence-verified fresh this
  round (`composer/terminals-tab.tsx`, `component/terminal-pane.tsx`,
  `component/dialog-shell-output.tsx`, server `handlers/pty.ts` + `pty-socket.ts`).
  (Nano N-c: these names are true-but-unpinned; see META.)
- **D4** PASS — writer-reader pairing bookkeeping explicit; longitudinal shapes in [G];
  schema data-driven; no carried enumeration.
- **E4** PASS — authored matrix; marker + p95 ×1.5 threshold; A3 content-boundary
  stated.
- **A1** PASS — state-set union authored-by-class (ratify-1c ruling upheld); ref token
  → element enumeration is the METHOD (runtime-derived), not a carried list.
- **D6** PASS — census pin re-executed exact (desktop/src = exactly 4; every named
  app/src subpackage exists); U10 absent-justified letter intact.

---

## META — whole-document coherence + independent re-execution of the STRENGTHENED guard (the R10 sweep)

**Verdict: CLEAN.**

1. **Discovery procedure re-runs mechanically and reproduces** — R10 §3.0 step 1's
   exact command yields **83**, with an ID sequence identical to the true clause
   universe (my independent enumeration; no stray wrapped-line IDs in v9 — the hazard
   R10 itself flagged is not present). The §3.1 table's 83 rows cover exactly these
   IDs, in order. **No clause is missed by the procedure; the mission's material-gap
   bar (a procedure-missed clause containing an uncensused enumeration) is NOT met —
   there is no such clause.**
2. **Full re-sweep executed with the strengthened unit** (every brace group, named-cell
   inventory, and count numeral extracted via R10's mechanical aid `grep -nE
   '\{[A-Za-z][^}]*[,}]'` minus gap-ID grammar, then per-clause intent adjudication):
   every ref-enumerating item lands in a guard-valid class. All 12 CENSUSED pins
   re-executed exact (method section); C13's new pin exact; C14's cure exact (PLUS
   below); every count annotation in R10 §4's numeric audit re-derived exact —
   including the fixed A6 (13 = 7+2+1+3, grep 17 = 13+4) and the tricky forms
   (merman core 15-by-find vs 14-by-ls because `color/` is a directory; `gitgraph/`
   lowercase dir carries the `gitGraph` kind's 7 files).
3. **Named-cell verifications re-executed**: B7 (ssh-askpass.ts, editor-zed* ×3,
   attention-sounds.{bun,node}), C1 (dialog-fork, anchors, anchor-view,
   message-navigation, grouping/{session,tree}), C16 (all 9 named tui files + the
   pierre family ×4 in session-ui) — all exist.
4. **Appendix + lineage**: 162/162 row-counted; bidirectional R10-# tags spot-checked
   (MG4→C14+I10b, SW1→I10b+C13, M-4→A6, R-G1→I10b, M-5→C20+I10b identity note);
   round-log v9 entry matches the header lineage and docs/spec-round10.md; clause
   numbering sequential; cross-references resolve (E5(b)→B2∪C16, E7→C5/D2/C18,
   C22↔B7/C4/F7/I8/F2/U8 boundaries, I2b→A5/C6/D3).
5. **Nano N-c (adjudication-strictness, non-blocking)**: three TRUE-but-unpinned named
   surface inventories sit inside rows R10 classed EXEMPT/illustrative — C10's UI
   parenthetical "(tokens sidebar, /connect, pair + QR)" (/connect = app/src/
   {servers,providers}/connect ✓, pair+QR = dialog-pair.tsx ✓, "tokens sidebar" = the
   settings/providers accounts-credentials UI in substance ✓); C15's composer tabs
   {shell, subagents, terminals} (all three files exist at
   `routes/session/composer/*-tab.tsx` — though R10 row 46's justification "tab files
   ride C16-family trees" is imprecise: they live in composer/, not the dialog trees);
   C17's four surfaces (all verified, S5). None is contradicted-by-ref (the MG-3/MG-4
   shape), none is a disagreeing numeral (the M-4 shape) — by the program's own
   calibration (C13 pre-R10: valid→pinned in-round, not a ratification failure) these
   are registration nanos, not material gaps. A future sweep may pin them
   NAMED-CELL-VERIFIED for symmetry.

---

## PLUS — regression grep over every cured class

**No stale claim survives; no cure regressed; nothing material introduced.**

- **MG-1 (telemetry family)**: every "no OTel implementation / no implementation in
  ref" occurrence (:719, :1048, :1107, :1340) is corrective narrative quoting dead
  wording; C22/F2/B7 cure texts re-verified line-exact (S4); ours greps still 0.
- **M-4 (A6 count)**: "all 12 listed panels" survives ONLY at :190 (the guard's
  defect-class example, quoting the error form) and :1379 (the appendix correction
  row). The A6 pin itself says 13 and the arithmetic re-executes (7+2+1+3; 17=13+4).
- **MG-3 (mermaid lists / A8)**: zero surviving 4-kind or TUI-inline-math claims
  ("inline vs block" appears only inside the v8 cure's own mis-attribution narrative
  and the appendix row); all seven A8 claims re-executed exact this round (method
  section), including the two count-form hazards.
- **MG-4 (C14 prompt source)**: "{command, agent, prompt}" survives at :29 (header
  cure narrative), :188 (the guard's escaping-form example), :603 (the R2 VERBATIM
  carry — mandated by I11, immediately followed by the v9 cure block), :1377
  (appendix) — exactly the C8-precedent structure. The cure's seven census claims all
  re-executed: config/plugin/ = 21 plugins, ZERO prompt entries; `grep Prompt
  {command,agent}.ts` = 0; `command.ts:159` glob (+ :146 comment); `agent.ts:22-23`
  globs with primary flags; `instruction.ts:38/:48/:72` AGENTS.md-only; decoy grep
  (`CLAUDE.md|cursorrules` in core/src/config/) = 0; MCP prompts runtime-served at
  `plugin/command.ts:17-27`; plugin API collections at `context.ts:128-129` =
  agent + command only.
- **Bonus (MG-2 class)**: no second 34-list; the 35-pin reproduces under this lane's
  execution.

---

## Register

| ID | Finding | Stance | Blocking? |
|---|---|---|---|
| — | (none material) | — | — |
| N-a | enterprise = a self-hostable share web app (SolidStart; routes/share/[shareID].tsx; core/{share,share-document,storage}) — a third member of the hosted share family (function/web), not merely "account"; U9's "account/identity/enterprise" label coarser than reality; disposition (not shipped by us) unaffected — M-5 class | S1 | no |
| N-b | stats = 3-sub-package analytics stack (app+core+server, each own package.json) incl. a viewer frontend; U8/C22-boundary wording "server-side stats/ only" is imprecise about the app leg; disposition accurate (we ship no stats-analytics surface; ours' session_stats.rs is the unrelated SessionStats family, ported) | S1 | no |
| N-c | three true-but-unpinned named-surface inventories classed EXEMPT-illustrative in R10's table (C10 UI parenthetical; C15 composer tabs — row-46 justification "ride C16-family trees" imprecise, files live in composer/; C17 four surfaces) — all verified TRUE this round; C13-style pinning would perfect the symmetry | META/S5 | no |
| N-d | protocol has no named ownership clause (consumers: server 30 / client 5 / cli 2 / plugin 1 / simulation 1 files) — transitively data-driven via D2/D1 + F2 accounting, same adjudication class R9/R10 gave D2; noted for the record | S1 | no |

## Overall call

**RATIFY-1‴: PASS.** All six lanes clean; the register carries only four
non-blocking nanos. Per docs/scan-program.md the next step is **RATIFY-2‴** on FINAL
candidate 4 (no integration round needed; counter does not reset on a pass).

Calibration: this is the first ratification round in the lineage (RATIFY-1: unowned
family; RATIFY-1′: 4-of-6 enumeration; RATIFY-1″: phantom enum axis + incomplete
sweep) whose META core — an independent, full re-execution of the guarding instrument
itself — found the guard's discovery procedure reproducible, its 83-row sweep
complete, and every census pin, cure, count annotation, and standing entry
line-exact against the ref tree. The R10 trajectory (unowned family → defective list
in one clause → defective axis the sweep skipped → guard strengthened + exhaustively
swept) has converged: what remains (N-a..N-d) is label-precision and
adjudication-symmetry polish, none rise to the material bar any prior round used.
The strongest remaining risk is monotonic: one more fresh-eyes pass (RATIFY-2‴) with
a different seed and package set is exactly what the program prescribes before LOCK.
