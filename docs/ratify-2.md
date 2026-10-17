# RATIFY-2‴ — Second and FINAL ratification round on FINAL candidate 4 (per docs/scan-program.md)

- **Under review**: docs/scan-spec.md **v9** (1385 lines; appendix 162/162; unchanged since
  RATIFY-1‴ — this lane judges the identical text with DIFFERENT angles, seeds, and
  package selections, per the two-clean-rounds-then-LOCK rule).
- **Lane**: `lane/ratify2` (worktree `.worktrees/lane-ratify2`). Stances S1-S5 + META,
  one full pass each, fresh eyes on v9, PLUS a first-execution simulation (3 random
  clauses dry-run verbatim). No clause edits applied (ratification judges; LOCK is a
  program-level act).
- **Verdict summary**: S1 CLEAN · S2 CLEAN · S3 CLEAN · S4 CLEAN · S5 CLEAN (10/10) ·
  META CLEAN. **Overall: RATIFY-2‴ PASS → v9 LOCKS as the 扫描细则**, the standing
  instrument for every future dev->scan->fix loop. Zero material gaps; register carries
  RATIFY-1‴'s four nanos (all adjudicated non-blocking, all re-verified true) plus three
  new non-blocking nanos from this round's fresh angles.

---

## Method + environment facts (measured this round, 2026-10-18)

- Ref tree `/opt/opencode-ref/packages/` = **35 packages** (`ls -1 | sort | wc -l` = 35,
  re-executed; list matches the I10b pin name-for-name). Ref repo git-pinned
  (`git rev-parse --short HEAD` = `e7a34f09bf`), root package.json version **2.0.24**;
  I2b binary `/root/.opencode/bin/opencode` runs and reports **v2.0.25** — the two-ref
  discipline's premises are live.
- **Guard discovery procedure re-run (the META core)**: `grep -oE '^(I[0-9]+b?|[A-F][0-9]+)
  (\*\*|[A-Za-z])' docs/scan-spec.md | wc -l` → **83**; extracted ID sequence = the true
  universe exactly (I1..I11 incl. I2b/I10b · A1-9 · B1-9 · C1-22 · D1-11 · E1-12 ·
  F1-7). R10 §3.1 row-count re-executed: **83 clause rows** (85 table lines incl.
  header/separator). Appendix re-counted row-by-row this round: R1 52 · R2 30 · R3 20 ·
  R5 12 · R7 15 · R8 11 · R9 17 · R10 5 = **162/162** — legend, header claim, and
  round-log chain (114 → 129 → 140 → 157 → 162) all consistent.
- **S5 sample**: `random.Random(41).sample` over the 83-clause universe in document
  order → **C18, C12, B8, A9, C19, E10, C6, E7, C5, F3** (reproducible; no overlap with
  RATIFY-1‴'s seed-37 sample). **PLUS draw**: `random.Random(2).sample` → **I7, I10b,
  I10** (reproducible).
- LIVE environment re-verified: tmux, sshd (loopback), docker, tc present; **mosh
  absent** (B9's DOCKER/RECIPE pin holds). /proc/PID/schedstat readable (E11).
- All ours-side instrument existence checks run **inside the worktree** (= committed
  state, a strictly stronger check than the main checkout).

---

## S1 — feature-space via the USER-JOURNEY lens (angle varied from all prior package-walk lanes)

Two full journeys walked against the ref tree's real surfaces (CLI handlers census,
TUI routes `home`/`session`, command registry, config discovery, dialog trees), every
step mapped to an owning clause:

**New user's first hour.** install (curl/npm/pnpm/bun/yarn/vp/brew) → **C11** (method
list census-pinned `updater.ts:14`) · first boot/onboarding (StartupLoading gates,
first-run seeding, MigrationOverlay — `tui/src/component/startup-loading.tsx` +
`migration-overlay.tsx` both exist) → **C12** + **E2**(a) + **I3** · config file
creation/precedence → **C4** (+ managed-policy.ts exists at
`core/src/managed-policy.ts`, U9 binding real) · provider auth/API key/model pick →
**C10** + **D9** (catalog/dialog cells) · first session: prompt→stream→tool permission
→ **C1** + **C3** (literal sets re-pinned) + **D1** · first plugin (marketplace) →
**C6** + **I2** + **A5** · first tab/session switcher → **C2** · theme try-out →
**A7** · notification/attention → **C3** · upgrade prompt → **C11**/C12 downgrade cell.

**Power user's day.** keymap customization → **B1** (user keybind overrides in [G]) ·
worktrees → **C8** · MCP add/auth/elicitation → **C9** · multi-client (TUI+TUI,
TUI+webui, flock race) → **F3** · terminals/shell tabs → **C17** (all four surfaces
existence-verified again) · share/export → **D8** · skills → **C13** ·
commands/agents/modes authoring → **C14** (v9 census cure text) · subagents → **C20**
tool + **C15** tab + **C1** states · search → **E12** · undo/revert (`session.undo`
command, `session.revert.messageID` at `routes/session/index.tsx:168-175,970-973` —
found this round) → **C1** (states enumerated from ref code, not prose — the revert
state is inside that discipline) + **F1** command matrix · timeline view
(`dialog-timeline.tsx`) → **C16** family · $EDITOR/zed → **B5** + **B7** named cells ·
ssh/docker/CI → **B9**/**B7** · headless scripting (`run` handler) → **C18** ·
`reload`/`service`/`stats`/`pair`/`acp`/`models`/`mini` CLI verbs → **C18** (census
re-executed: 18 non-test handler entries, exact) + C10/D7/D9/A6/U8 · codemode →
**D10** · desktop/webui → **D6** (census re-pinned: desktop/src = exactly {main,
preload, renderer, shared}) · telemetry envs → **C22**.

**Verdict: CLEAN** (2 nanos, both in the owned-in-substance/unpinned-in-letter class):

| Journey finding | Reality (verified this round) | Ownership | Blocking? |
|---|---|---|---|
| J-a compat skills roots | `core/src/config/plugin/compatibility.ts` (internal plugin `opencode.config.compatibility`): scans `<compat-root>/skills/{*.md,**/SKILL.md}` from `.claude/`+`.agents/` roots (config discovery's `sources.claude/.agents`, `discovery.ts:22-27`), delivers via `ctx.skill.transform`, live-watched. Ours: zero `.claude`/`.agents` compat greps in crates/ | **C13** owns the skills family (directory-shaped source, discovery phase, [F] real skill dir); the ROOT axis is unnamed; C13's pin parenthetical ("paths/urls at normalize.ts:243-244 — verified exact —; embedded via plugins") is incomplete-but-true, and the pin's EXACT claim is only about the `type Source` union (`skill.ts:45` re-verified exact). Nothing contradicts ref → N-c class (label precision), not MG | no |
| J-b live re-discovery axis | config plugins `source/command/agent/compatibility` subscribe `config.changes()` + filesystem watchers (parent-watch for delete/recreate) → live catalog reload, 100ms debounce (`compatibility.ts:64`) | Discovery STATE = C13/C14 phases; the live-transition axis unnamed; the debounce TIMING is literally **I3** ("debounces" is a named member). Implicit-but-real coverage, A4-clock-note class | no |

No journey step was found without an owning clause; no false premise about any surface.

---

## S2 — quality (E-executability spot re-verify, 5 clauses: E1, E3, E5, E11, E12)

**Verdict: CLEAN.** All five carry artifact + threshold + instrument verbatim and every
named instrument exists in the committed worktree: E1 stall-burst rule + histogram
JSON (`frame_budget_tests.rs`, `replay.rs` in-tree) · E3 slope ≤ max(2×ref, ref+20 MB/h)
+ /proc sampling (executable on this host; F5 harness shared) · E5 4.5:1/3:1 + ≥ref
per cell with target enumeration = B2∪C16 (both clauses exist and are themselves
enumerated) · E11 CPU ≤ ref+0.5%, wakeups ≤ ref×1.5, byte-rate=0 (schedstat verified
readable; pidstat fallback stated) · E12 committed corpus + rank-order parity
(`e12_search_corpus.json` + `spec_e12_first_pass.rs` both in-tree at the pinned path).
E2's two-leg structure re-confirmed with `tools/parity/spec-p3/refbin_tti_results.json`
committed (ours leg honestly pending — the clause predicts exactly this state).

## S3 — environment (B/C riders spot re-verify: B3, B6, B7, B8, B9, +C7/E6)

**Verdict: CLEAN.** All riders present verbatim: B6 `:406`, B7 `:434`, B8 `:463-464`,
B9 `:481`, B3 R8-N4 `:381`, C7 R8-N3 `:539`, E6 R8-N1 `:935`. (Note: a single-line
grep for "R7 executability rider" yields 3 because B8's tag wraps across two lines —
counted 4/4 by content; a wrap artifact, not a miss.) LIVE dispositions re-verified
against the host (tmux/sshd/docker/tc LIVE; mosh absent → recipe cell); B7's OTEL_*
cell set and B7↔C22 boundary intact.

## S4 — fidelity (instrument-binding spot)

**Verdict: CLEAN** (1 nano). Every R7-extension instrument exists at its pinned path
in the worktree (committed state): `spec_e1_canary.rs`, `spec_e5_canary.rs`,
`spec_e2_tti.rs`, `spec_e12_first_pass.rs`, `e12_search_corpus.json`,
`e3_leak_canary.rs`, `rescan-tui.rs`, `spec-p3/` (+ `refbin_tti_results.json`), ref
`packages/simulation`. The five BASE instruments exist too — under
`crates/oc-tui/src/` (module tests), not `tests/`: `golden_tests.rs`,
`frame_budget_tests.rs`, `replay.rs`, `mouse_modes.rs`, `diff_viewer_golden_tests.rs`
all present. Nano **I5-n**: I5 binds the base five by bare name, and `golden_tests.rs`
/`replay.rs` each have three in-tree namesakes (`src/`, `src/components/composer/`,
`src/components/footer/`; replay also `oc-cli/src/acp/`) — resolvable by content (the
originals predate the component splits), but a first-execution agent must
disambiguate. Registration nano, non-blocking.

## S5 — granularity/adversarial (10 clauses, seed 41, letter-vs-spirit)

**Verdict: CLEAN — 10/10 PASS.**

- **C18** PASS — handler census re-executed exact (18 non-test entries, list identical
  to the pin incl. nested `auth/ debug/ mcp/ plugin/ service/ session/`); every cell
  executable.
- **C12** PASS — first-run shapes authored; StartupLoading/MigrationOverlay surfaces
  existence-verified; gates I3-pinned (E2's thresholds materialize them).
- **B8** PASS — pass criterion (cursor-cell position, not version strings) is
  executable; ours side is real (`unicode-width = "0.2"` in oc-tui/oc-cli Cargo.toml,
  with an honest pin comment about the dual-version lock); canary DESIGNED per the I8
  tier (no B8 evidence exists — the matrix's predicted state).
- **A9** PASS — ours image family is non-vacuous (composer/attachment.rs,
  oc-schema/config/media.rs) and ref's cells exist (mini stream-v2.image in A6's
  census; diff-viewer in I5's instruments); absent-justified per missing renderer.
- **C19** PASS — `integration-id.ts:3` re-verified = branded Schema.String exactly;
  runtime-enumeration method (integration.list on the I2b leg) executable — binary
  runs (v2.0.25).
- **E10** PASS — thresholds + monotonic rule + write-amplification artifact; /proc/io
  metrics executable; shares E3/F5 harness.
- **C6** PASS — three-layer state assert + I2b parity leg executable; artifact table
  specified.
- **E7** PASS — error-path union sources (C5/D2/C18) all exist; placeholder corpus is
  NON-VACUOUS for ours: `grep 'Not wired in this build' crates/` = **4 live hits** —
  the clause has real work to do at first execution (exactly what a standing
  instrument should have).
- **C5** PASS — `dialog-workspace-file-changes.tsx` exists (component tree, inside
  C16's enumeration; C5 names it without a path — no premise break); kill/fuzz method
  executable.
- **F3** PASS — two-live-client cells; webui leg disposition rides D6; flock/SSE
  isolation asserts concrete.

---

## META — N-a..N-d adjudication + whole-document coherence after ten rounds of edits

**Verdict: CLEAN.**

1. **N-a (enterprise)**: reality re-confirmed (`enterprise/src/routes/share/[shareID].tsx`
   + `routes/share.tsx` exist — a self-hostable SolidStart share web app, third member
   of the hosted share family). **Truly non-blocking**: U9's disposition (not shipped
   by us) is unaffected by the finer label; the function/web entries' "future client
   re-baselines per I10" rule already covers a hypothetical port; no scan cell changes
   owner. RECORD.
2. **N-b (stats)**: reality re-confirmed (stats/ = app+core+server, 3 package.jsons).
   **Truly non-blocking**: we ship no analytics surface; ours `session_stats.rs`
   (existence re-verified) is the unrelated ported SessionStats family owned by
   C18/D-cells; U8's wording imprecision changes no disposition. RECORD.
3. **N-c (three unpinned inventories)**: all three re-verified TRUE this round (C15
   composer tabs — `shell-tab/subagents-tab/terminals-tab.tsx` exist; C17 four
   surfaces; C10 connect/pair/credentials). **Truly non-blocking**: nothing is
   contradicted-by-ref (MG shape) and no numeral disagrees (M-4 shape); by the
   program's own C13-precedent these are registration items, perfectible by a future
   NAMED-CELL-VERIFIED pass. RECORD.
4. **N-d (protocol unnamed)**: **Truly non-blocking**: protocol is the HttpApi
   definition layer that materializes AS D2's endpoints and D1's events at runtime;
   client.ts is D11's .d.ts shape-diff target; middleware = D2 error paths;
   simulation.ts = the I5-bound tapes. Every file family has a consuming clause; same
   adjudication class R9/R10 gave D2 (runtime-named). RECORD.
5. **Whole-doc coherence**: clause numbering sequential and complete (83-universe
   discovery re-run, above); every cross-reference resolves (E5(b)→B2∪C16, E7→C5/D2/
   C18, C22↔B7/C4/F7/I8/F2/U8, I2b→A5/C6/D3, C14→C9/C20/F7, C8→F7, E3/E10/E11→F5/E1,
   D9→I10, A2→A1/B8, I7→I10b/F2); appendix bidirectional for all five R10 tags
   (MG-4→C14, SW1→I10b+C13+C20, M-4→A6, R-G1→I10b, M-5→C20+I10b identity note);
   lineage header ↔ round-log v9 ↔ docs/spec-round10.md consistent; the verbatim
   integration rule (I11) visibly honored (R2 §c/R3 §5/R8/R9/R10 texts carried).

---

## PLUS — first-execution simulation (3 random clauses, seed-2 → I7, I10b, I10)

All three **executable as written** against our tree:

- **I10b** — executed: the pinned census command yields 35/35 exactly; the (a)/(b)
  report-lists requirement is mechanically checkable. EXECUTABLE.
- **I10** — executed: ref is git-pinnable (`e7a34f09bf`), version 2.0.24 in
  package.json, binary 2.0.25 runs — a round report can record the pin today.
  EXECUTABLE.
- **I7** — executed as a real slice: `packages/theme` enumerated mechanically (10
  files under src/tui; 24 exports extracted by grep: expandTheme/expandTokens/
  generateSyntax/mergeTheme/migrateV1/oklchToHex/parseThemeDocument/resolveTheme/
  resolveThemeDocument + 15 type/schema defs); ours counterpart greppable
  (`crates/oc-tui/src/theme.rs`, 35 pub items) — every export classifiable as
  ported/absorbed/absent-justified. The completeness backstop runs exactly as its
  letter says. EXECUTABLE. (The accounting itself is scan-round work, not
  ratification's — the simulation only proves the clause is executable, which it is.)

No clause that cannot be executed as written was found.

---

## Register

| ID | Finding | Stance | Blocking? |
|---|---|---|---|
| — | (none material) | — | — |
| J-a | compat skills roots unnamed: `.claude/skills` + `.agents/skills` discovered by internal plugin `opencode.config.compatibility` (`config/plugin/compatibility.ts`, visible but unannotated in R10 §3.0's own plugin-dir census); ours ships none; C13 owns the family, the root axis + C13 pin parenthetical are incomplete-but-true (N-c class) | S1 | no |
| J-b | live re-discovery axis (config-plugins' watchers + config.changes(), 100ms debounce) unnamed across C13/C14; state covered by discovery phases, timing covered by I3's "debounces" (A4-clock-note class) | S1 | no |
| I5-n | base-instrument binding is by bare name; `golden_tests.rs`/`replay.rs` have 3 in-tree namesakes each (src/ vs component splits) — first-execution agent must disambiguate by content | S4 | no |
| N-a..N-d | RATIFY-1‴'s four nanos re-verified true and adjudicated non-blocking (META 1-4) | META | no |

## Overall call

**RATIFY-2‴: PASS.** All six lanes clean under deliberately varied angles
(user-journey walk, seed-41 sample, committed-state instrument check, first-execution
simulation); the register adds only three non-blocking nanos of the same
registration/label-precision classes the program has consistently ruled sub-material.
Per docs/scan-program.md's post-final ratification rule — RATIFY-1‴ PASS + RATIFY-2‴
PASS — the spec now **LOCKs: docs/scan-spec.md v9 is the 扫描细则**, the standing
instrument for every future dev->scan->fix loop. The locked text is FINAL candidate 4
(commit f81eab2 text, unchanged by either ratification round); J-a/J-b/I5-n and
N-a..N-d remain in the ledger for the first post-LOCK maintenance window (pinning them
is an integration-round act, not a ratification failure — the C13 precedent).
