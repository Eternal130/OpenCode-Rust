# RATIFY-1″ — Fresh first ratification round on FINAL candidate 3 (per docs/scan-program.md)

- **Under review**: docs/scan-spec.md **v8** (1246 lines; appendix 157/157; SPEC-R9 cures
  for RATIFY-1′'s MG-3 declared in docs/spec-round9.md; FINAL candidate 3).
- **Lane**: `lane/ratify1c` (worktree `.worktrees/lane-ratify1c`). Stances S1-S5 + META,
  one full pass each, fresh eyes on v8, PLUS a regression grep over every cured class.
  No clause edits applied (ratification judges; the material finding below is proposed
  for R10).
- **Verdict summary**: S1 CLEAN · S2 CLEAN · S3 CLEAN · S4 CLEAN · S5 MATERIAL-GAP ·
  META MATERIAL-GAP (same root; 2 cosmetic residuals).
  **Overall: RATIFY-1″ FAIL** → R10 integration, convergence counter resets again.
  One material gap: **MG-4 C14(b)'s source enum {command, agent, prompt}
  over-enumerates ref — `prompt` has NO markdown/discovery source in ref** — an
  uncensused R2-carried list that the v8 list-sweep itself missed (the new
  list-census guard does NOT fully hold under independent execution).
  The R9 cures themselves verified SOUND and line-exact throughout (A8 re-executed
  100%: 6 kinds, 7 dir counts, 4 sub-families, both math stacks, renderer universe,
  ours-grep = 0); the one new defect the cures introduced is a cosmetic count
  annotation inside A6's own census pin (12 vs 13).

---

## Method + environment facts (measured this round, 2026-10-17)

- Ref tree `/opt/opencode-ref/packages/` = **35 packages**; the I10b census command
  reproduces the pin exactly (`ls -1 … | sort` sorted-diff vs the 35-name pin list =
  empty — an initial non-empty diff in my harness was a trailing-newline bug in my
  own comparison script, re-run clean; `wc -l` = 35; 6 exhaustive + 29
  family-granular arithmetic exact). Ref binary `/root/.opencode/bin/opencode`
  (2.0.25) per I2b.
- **Every v8 census pin re-executed independently** (this is the META core): I10b
  35/35 ✓; A5 slots = exactly {home, prompt, sidebar, system} ✓; A6 files ✓ (but the
  pin's count annotation is wrong — see M-4); A8 all seven claims exact
  (diagnostics.ts:1 union of 6; per-kind dir counts 12/10/18/7/7/7 + core 15;
  markdown.ts/palette.ts (`resolveOpenCodeDiagramPalette` :46)/plugin.ts
  (builtins.ts:13)/layout-audit (`script/layout-audit.ts` + tests); latex plugin.ts:11-12
  two registrations; marked-parser inlineKatex :17/blockKatex :41 with displayMode
  false :35 / true :52; renderer-universe grep = merman + latex only (the 3 other
  hits are the registration API/declaration, not registrations); KaTeX 0.16.47
  pinned in `ui/package.json` :117; limits 100_000/256 at limits.ts:3-4; the 5
  symbol tables all in symbols.ts; `grep -ri 'mermaid\|merman\|latex\|katex'
  crates/` = **0 total hits**); C3 literals exact at permission.ts:41/:55 ✓; C7
  ToolState union exact at session-message.ts:127/:133/:140/:148 (the three extra
  grep hits :249/:258/:271 are the sibling **Compaction** union — correctly outside
  the tool-part pin) ✓; C8 = exactly four `Effect.fn("Worktree.…"` + `session_move`
  at opencode.ts:110 ✓; C11 updater.ts:14 = the exact 7-method array ✓; C16 = 23 ∪ 4
  dirs ✓; C18 handlers all 18 named entries present ✓; C19 branded string at
  integration-id.ts:3 ✓; C20 tool dirs = 6 + 15 exact ✓; D6 desktop/src = exactly
  {main, preload, renderer, shared} + all named app/src subpackages exist ✓; D9
  Variant = untyped overlays in model.ts:113-116 ✓.
- S5 sample: `random.Random(29).sample` over the **83-clause universe** in document
  order → **E7, I9, C14, F1, F2, C6, I10b, E2, F5, C20** (reproducible; zero overlap
  with RATIFY-1′'s seed-23 sample; honest note: **F1 also appeared in RATIFY-1's
  seed-11 sample** — the mission-prescribed seed 29 yields it anyway; F1 was
  re-judged fresh against v8 text).
- B6/B9 LIVE environment tools present: tmux, sshd (loopback), docker, tc ✓.

---

## S1 — feature-space (8 assigned packages: client, containers, script, storybook, theme, session-ui, identity, effect-drizzle-sqlite)

**Verdict: CLEAN** (1 residual + 1 nano). Fresh reality-check per package:

| Package | Reality (verified this round) | Owned? |
|---|---|---|
| client | 64-file client library: effect/promise/solid sub-clients, service discovery (service/contender/probe/timing/version), pty-handoff, shared-events, contract; consumers = app, sdk, session-ui, server, storybook, enterprise, plugin, tui, cli | ✓ **D11** names `packages/client` .d.ts shape-diff explicitly; consumer-driven letter fits |
| containers | 5 Dockerfiles (base, bun-node, rust, tauri-linux, publish) + script/build.ts + README — build/CI recipes, no runtime surface | ◐ accounting class (RATIFY-1 adjudicated; rides B7/B9 docker cells) — residual R-G1 |
| script | 4 files; index.ts = release/version/channel bump tooling (OPENCODE_CHANNEL/BUMP/VERSION/RELEASE reads) | ✓ I10b standing entry accurate ("single-file dev script — F2 accounting") |
| storybook | 31 files: .storybook config (main/manager/preview/extension/theme-tool) + mocks/{app,gui-extensions}; no src/ product code | ◐ accounting class (RATIFY-1 adjudicated) — residual R-G1 |
| theme | theme/src/tui/{select,resolve,expand,syntax,color,schema,types,v1,v1-migrate,index}.ts + build/publish scripts; consumers = tui + plugin | ✓ **A7** 1:1 (discovery→select→apply chain = select/resolve/expand; oklab in color; syntax themes in syntax.ts; v1-migrate = C12/C4 migration leg); A1 owns the tokens |
| session-ui | 180 files: src/{actions, components (55 tsx), context, document, file-presentation, message, pierre, timeline, tools, v2, storybook}; consumes `@opencode/ui`'s marked-parser (`session-ui/src/components/markdown.worker.ts:4`) | ✓ exhaustive pin + **A8(b)** (the KaTeX/markedShiki leg) + **C16** third tree + pierre review-mode family verified exact (commented-lines/comment-hover/diff-selection/file-find all present in session-ui/src/pierre/) |
| identity | 6 brand-mark files only (mark.svg + PNG variants) — no code at all | ✓ U9 OOC entry valid ("not shipped by us"); nano: the U9 label "account/identity" is coarser than the reality (logo assets) — disposition unaffected |
| effect-drizzle-sqlite | 2 files (sst-env.d.ts + one example migration.sql); no src; sole importer = core TEST (`core/test/database-drizzle.test.ts`, re-verified) | ◐ accounting class (RATIFY-1 adjudicated) — residual R-G1 |

### R-G1 (residual): the standing-accounting-entry ledger is asymmetric — the R-E cure fixed the named pair, not the class

R9's R-E cure added standing I10b entries for `function` + `web` (the pair ratify-1b
named). But **containers, storybook, effect-drizzle-sqlite, httpapi-codegen,
http-recorder** sit in the identical structural position — no parity surface, no
clause names them, no standing entry — their accounting-class adjudication lives
only in docs/ratify-1.md's table, not in the spec text. Every F2 round must
re-adjudicate them from scratch, which is exactly the friction R-E existed to
remove. Non-blocking for the same reasons ratify-1b gave R-E: no false premise
exists anywhere about these packages, the census pins them, F2's letter reaches
their (trivial) exports. Proposed cure (R10, NOT applied): five one-line standing
entries, same format as function/web.

---

## S2 — quality (E-clause executability, incl. the new C7/C9/A4 riders)

**Verdict: CLEAN.**

- All 12 E-clauses re-checked against the v8 text: artifact + threshold intact
  everywhere (E1 stall-burst ✓, E3 max(2×ref, ref+20 MB/h) ✓, E4 p95 ×1.5 N=20 ✓,
  E5 4.5:1/3:1 + ≥ref-per-cell + B2∪C16 enumeration ✓, E6 R8-N1 format-string
  equality ✓, E7 placeholder scan + exit-code completeness ✓, E8 self-canary-ing
  per RATIFY-1 precedent ✓, E9 R8-N2 residue audit ✓, E10 max(2×ref, ref+50 MB/h)
  + monotonic rule ✓, E11 CPU/wakeup/byte-rate ✓, E12 ordered result-list
  equality ✓). E2's two legs + binary-gating unchanged; measured numbers
  consistent clause↔round-log (5250/5300 source-dev, 2300/2400 binary); no ratio
  verdict claimed anywhere (ours leg still pending, recipe committed).
- **C7 rider + v8 census pin: landed and exact.** R8-N3 artifact/assert intact; the
  M-3 census pin's four line cites reproduce exactly and scope correctly to the
  ToolState union (the extra grep hits at :249/:258/:271 are the Compaction union —
  a different family; the pin's line-cite discipline disambiguates, verified).
- **C9 v8 threshold note: landed** ("every cell completes its round-trip with
  matching observable state on both stacks") — closes ratify-1b M-3's second leg.
- **A4 v8 clock note: landed** (vt-clock drives OUR leg; ref captured at pinned
  REAL cadences; comparison at the same named cadence sources) — closes M-3's
  third leg. All three ratify-1b M-3 cosmetics cured.

---

## S3 — environment (B6-B9 + the OTEL cells)

**Verdict: CLEAN.**

- **B6** unchanged-and-complete (8 row-classes, two-tier threshold, LIVE/DOCKER/
  DEFERRED-WITH-RECIPE dispositions; cell-table artifact). **B8** complete
  (corpus runner, cursor-cell criterion, zero-desync, canary-before-evidence with
  tier order intact — still no B8 evidence exists, so NONE-DUE/OVERDUE stay
  consistent). **B9** complete (6 transports dispositioned; echo-latency p50/p95,
  redraw-storm ≤ ref×2, failure-or-degrade asserts). LIVE tools re-verified
  present on host (tmux, sshd, docker, tc).
- **B7 + OTEL_***: the **M-2 consistency cure landed** — the v6 LIVE list now
  names OTEL_* with the tagged v8 addition (:377-378), closing ratify-1b M-2. The
  R8 OTEL_* cell set remains complete against C22's env-read set: all 3 vars
  covered (ENDPOINT set; HEADERS valid+malformed; RESOURCE_ATTRIBUTES
  valid+malformed — re-verified against ref: `parseHeaders` never throws,
  `resourceAttributes()` at observability/otlp.ts:30-36 catches/drops). The
  E6↔B7-style boundary (B7 = boot/survival; C22 = observability behavior,
  incl. the ours-today no-reads case) is stated and correct.

---

## S4 — fidelity (instrument-layer binding; I5's C22 mock collector)

**Verdict: CLEAN.**

- **R-F cure landed and honest**: I5's v8 extension names C22's mock OTLP collector
  as a standing instrument with the explicit rule "the first C22 execution commits
  it in-tree" — and no collector exists in-tree yet, exactly as the rule predicts
  (grepped tools/ + crates/ for v1/logs / mock-otlp: zero). The residual
  ratify-1b flagged is cured as proposed; no overclaim added.
- **All other I5 instruments re-verified file-by-file in-tree**: golden_tests.rs,
  frame_budget_tests.rs, replay.rs, mouse_modes.rs, diff_viewer_golden_tests.rs,
  spec_e1/e5/e2/e12 canaries + e12_search_corpus.json + e3_leak_canary.rs,
  rescan-tui.rs, tools/parity/spec-p3/ (incl. refbin_tti_results.json), ref
  packages/simulation. Zero ad-hoc rebuild drift.
- **C22 spot re-verification** (key claims, line-exact): `cli/src/index.ts:8`
  (Observability import) and `:126` (`Observability.layer({`) ✓; env-read set =
  exactly the 3 vars (observability.ts:22-23, observability/otlp.ts:30) ✓; config
  key `openTelemetry` at core normalize.ts:49 ✓; ours side: `grep OTEL_\|otlp
  crates/ --include="*.rs"` = 0 ✓. The `{.ts,/otlp.ts}` brace paths resolve
  correctly to observability.ts + observability/otlp.ts (both exist).
- I8 canary-coverage matrix unchanged and coherent (C22 design addendum intact;
  defer-or-design rule unbroken).

---

## S5 — granularity (10 clauses, seed 29, letter-vs-spirit)

**Verdict: MATERIAL-GAP (MG-4, in C14).** Nine of ten PASS:

- **E7** PASS — enumeration derived from three censused/bounded sources (C5/D2/C18);
  placeholder corpus is authored (guard-exempt); copy-diff + exit-code table
  executable.
- **I9** PASS — provenance rule, no list carried.
- **F1** PASS — matrix runner + per-axis coverage % + I8 canary on the matrix
  (re-judged fresh despite the seed-11 overlap; v8 text unchanged-sound).
- **F2** PASS — export accounting vs the I10b universe; all standing entries
  re-verified against reality this round (devtools, util/observability→C22, plus
  S1's package checks).
- **C6** PASS — three-layer state assert + I2b parity leg intact.
- **I10b** PASS — the pin itself: census reproduces exactly (see method); guard
  text coherent in intent (its execution gap is MG-4, judged under META).
- **E2** PASS — see S2.
- **F5** PASS — soak umbrella; E3/E10/E11 name it as their shared harness
  (circularity is explicit and coherent, each metric leg carries its own
  threshold).
- **C20** PASS — census pin exact (6 + 15 entries verified); [G] covers every
  census entry (AGENTS.md in the tool dir is a doc file, not a tool — the only
  un-named census entry, nano).
- **C14** **FAIL** — see MG-4.

### MG-4 — C14(b)'s source enum {command, agent, prompt}: `prompt` is not a ref source kind

Evidence (all fresh, this round):

1. **Ref's command discovery**: `core/src/config/plugin/command.ts:159` — glob
   `{command,commands}/**/*.md`. **Agent discovery**:
   `core/src/config/plugin/agent.ts:22-23` — globs `{agent,agents}/**/*.md` and
   `{mode,modes}/*.md` (modes are primary agents). There is **no prompt analogue**:
   `core/src/config/plugin/` contains command/agent/skill/skill-file/instruction
   plugins and **no prompt source plugin**; `grep -rn "Prompt"
   core/src/config/plugin/{command,agent}.ts` = 0 hits.
2. The only "prompts" in ref are **MCP prompts** (`core/src/plugin/command.ts:17-35`,
   `Mcp.Prompt[]`, server-provided) — a C9/C20-adjacent runtime surface, not a
   `.opencode/` markdown source; and the plugin API registers **agent + command
   collections only** (`plugin/src/tui/context.ts:128-129`; its "prompt" hits are
   the prompt.footer composer slots and the `prompt()` ask-dialog — different
   family). The posts/docs site carries no prompt-source convention.
3. **C14's own [F] leg betrays the [G]**: "[F] real `.opencode/{command,agent}/*.md`
   shapes" — command and agent only. The [G] axis "source enum {command, agent,
   prompt}" (scan-spec.md:536-537) is the R2 S1-2 wording carried verbatim
   R2→v8, never censused.
4. **The v8 list-sweep missed it**: docs/spec-round9.md §2's sweep table adjudicates
   26+ clauses — C14 is not among them (nor A1/B4/C1-adjacent lists, which I
   independently verified true or exempt: I3's cadence values all exact — spinner
   80 `spinner.tsx:21`, StartupLoading 500/3000 `startup-loading.tsx:46/:22`,
   backoff 1s/5s `app.tsx:1313`; C1's named cells all exist — dialog-fork.tsx,
   anchors.ts, anchor-view.tsx, grouping/, message-navigation.ts; C13's three
   skill sources all real — skills.paths/urls at normalize.ts:243-244,
   EmbeddedSource at plugin/skill.ts:18; B4/A2/A9 authored-by-class).

**Guard ruling**: under I10b's v8 list-census guard as written — "any clause that
CARRIES a list purporting to enumerate ref reality … must cite the MECHANICAL
census command … A carried list without a census citation is the MG-3 defect class
… and FAILS ratification on sight" — C14(b) is precisely such a list, and it is
additionally **contradicted by ref source** on one axis (the same shape as C8's
{adopt, rename}, which R9 itself judged cure-worthy, and MG-3's 4-of-6). This is a
sweep-completeness failure of the guard's first execution, not a flaw in the
guard's design: the guard's letter holds; R9's sweep simply didn't enumerate every
clause.

**Materiality** (honest calibration): narrower than MG-1, same class as MG-3/C8-L4.
No live divergence, no unowned family (C14 owns commands/agents/instructions); the
[F] leg contains practical damage to command/agent shapes. But an agent
materializing C14(b)'s [G] spawns a phantom prompt-source discovery axis with no
ref counterpart (vacuous cells, or mis-reported divergence on a surface ref does
not have), and the program's own v8 standard — enacted this very round — rates an
uncensused contradicted carried list as fail-on-sight. **Proposed cure (R10, NOT
applied):** C14(b) census pin — commands = `{command,commands}/**/*.md`
(config/plugin/command.ts:159); agents = `{agent,agents}/**/*.md` ∪
`{mode,modes}/*.md` (agent.ts:22-23, modes = primary); `prompt` re-dispositioned
(no markdown source in ref; MCP prompts are runtime-served → C9/C20-owned cells,
ours-extra row under F7 if we ever ship one).

---

## META — whole-document coherence + the NEW list-census guard's self-consistency

**Verdict: MATERIAL-GAP** (MG-4 is a META failure too — the guard did not hold
under this lane's independent execution; plus 2 cosmetics).

- **Appendix arithmetic exact (row-counted this round)**: R1 52 · R2 30 · R3 20
  (12+8) · R5 12 · R7 15 · R8 11 · R9 17 = **157/157**; header, legend, and
  round-log chain (102 → 114 → 129 → 140 → 157) all consistent.
- **All 12 v8 census pins re-executed: every one reproduces its claim EXACTLY —
  except A6's count annotation** (M-4 below). The guard's census-command format is
  genuinely mechanical (I could run every one verbatim).
- **The guard's first execution (R9's sweep) is incomplete**: it adjudicated a
  hand-picked 26+ clause set, not the full 83-clause universe; C14 escaped and is
  MG-4. Secondary sweep-adjacent lists independently verified true/exempt: I3
  cadences (all four values exact), C1 named cells (all exist), C13 sources (all
  three real), C16 named dialogs (dialog-stash/experiments/debug/status/
  shell-output, masked-text, retry-provider, pierre family — all exist), C10
  surfaces (dialog-pair, use-connected exist), A1/B2/B4/A2/A9 authored-by-class
  (same rulings R9 gave C9/C13/C15/C17).
- **R9 riders all landed and coherent**: R-E standing entries present and accurate
  (function/web verified against binary `--help` in ratify-1b; wording unchanged);
  R-F in I5; M-1 REG letter corrected in the appendix row (in place, with note);
  M-2 in B7's LIVE list; M-3 in C7/C9/A4. Round-log v8 entry matches the header
  lineage and the R9 report.
- **Cross-references resolve** (spot-set re-checked): E5(b)→B2∪C16; E7→C5/D2/C18;
  I2b→A5/C6/D3; E6↔B7 and C22↔B7 boundaries; C22→C4/F7/I8/F2/U8; F2/I7→I10b;
  C14(a)→instruction hierarchy real (instruction.ts global+project+ancestor walk).
  Clause numbering sequential (A1-9, B1-9, C1-22, D1-11, E1-12, F1-7, I1-I11
  incl. I2b/I10b).
- **Cosmetic M-4**: A6's census pin says "all **12** listed panels exist as files"
  — the clause's own enum lists **13** (footer.{command,form,menu,permission,
  prompt,subagent,view} = 7, scrollback.{writer,surface} = 2, splash = 1,
  stream-v2.{fragment,image,subagent} = 3), and all 13 exist (the grep yields 17
  files = 13 panels + 4 plumbing/shared: footer.ts, footer.width.ts,
  scrollback.shared.ts, stream-v2.transport.ts). The enumeration and the census
  command are exact; only the summary numeral is off by one — an ironic defect
  class (a count error inside a census pin), but not list-contradicting: an agent
  materializing from the clause gets the correct 13. Same register as R9-M-1
  (cosmetic). Proposed cure: "12" → 13.

---

## PLUS — regression grep over every cured class

**No stale claim survives; nothing material introduced by the cures except M-4.**

- **MG-1 (telemetry)**: all "no implementation/no OTel" occurrences (:616, :945,
  :1004, :1211) are corrective narrative quoting dead wording; C22/F2/B7 cure
  texts re-verified line-exact (S4).
- **MG-2 (package census)**: no second copy of the old 34-list anywhere (:145,
  :1212 are the correction narrative); pin reproduces 35/35 under this lane's
  independent execution.
- **MG-3 (A8)**: zero surviving 4-kind-union or TUI-inline-math claims; the v8
  rewrite verified 100% against source (method section). The R2→v7 carry pattern
  is broken for A8 — but **survives in C14(b)** (MG-4), which is the finding.
- **C8 (worktree)**: the [G] still carries the R2 verbatim op list (:486) — but the
  in-clause v8 census cure (:490-498) explicitly re-dispositions adopt/rename to
  F7 ours-extra rows; the verbatim-carry + appended-cure structure is the I11
  integration rule working as designed; no live stale claim.
- **A8-adjacent ours-claims**: exact spec grep over crates/ = 0 hits (not merely 0
  code hits) — the absent-justified premise holds with margin.
- **auditA/~120, E5-v4 subjective check, D9/D10 number-reuse**: all remain
  deleted/narrative-only.

---

## Register

| ID | Finding | Stance | Blocking? |
|---|---|---|---|
| MG-4 | C14(b) source enum {command, agent, prompt} over-enumerates ref: `prompt` has NO markdown/discovery source (config/plugin has command.ts `{command,commands}/**/*.md` :159 + agent.ts `{agent,agents}` ∪ `{mode,modes}` :22-23, no prompt plugin; plugin API registers agent+command only; MCP prompts are runtime-served, C9/C20-class); R2-carried verbatim, never censused; **missed by the v8 list-sweep — the guard's first execution was incomplete** (26+ clauses adjudicated, not 83); [F] itself exercises only {command,agent} md shapes | S5/META | **MATERIAL** |
| R-G1 | I10b standing-entry ledger asymmetric: containers, storybook, effect-drizzle-sqlite, httpapi-codegen, http-recorder adjudicated accounting-class in ratify-1.md but carry no standing entries (unlike console/posts/script/function/web) — R-E cured the named pair, not the class | S1 | no |
| M-4 | A6 census pin count annotation wrong: says "all 12 listed panels", the clause lists 13 and all 13 exist (grep = 17 files = 13 panels + 4 plumbing); enumeration + command exact, numeral off by one — inside a census pin | META | no |
| M-5 | nano: U9's "account/identity" label is coarser than identity's reality (6 brand-mark asset files, zero code); disposition valid regardless. C20's census `ls core/src/tool/` also yields AGENTS.md (a doc, unnamed in the pin's enumeration) | S1/S5 | no |

## Overall call

**RATIFY-1″: FAIL.** One material gap (MG-4). Per the program: route to **R10
integration** (C14(b) census pin + prompt-axis re-disposition; A6 numeral fix;
optional R-G1 class-cure: five standing entries + a sweep-completeness rule that
the list-sweep table itself enumerates all 83 clauses), counter resets, then a new
FINAL candidate before the next ratification round.

Honest calibration for R10: MG-4 is the same register as MG-3 — zero live
divergence, no unowned family, practical damage contained by C14's [F] leg — but
it fails the standard v8 itself enacted ("uncensused carried list = MG-3 class,
fails on sight"), and it demonstrates that the guard's process cure needs one more
iteration: the sweep that enforces the guard must itself be exhaustive, not
curated. Everything else v8 touched verified exact under independent re-execution
— every census pin, every rider, every number — and no cured class regressed.
The trajectory is converging: RATIFY-1 found an unowned family, RATIFY-1′ a
defective enumeration in one clause, RATIFY-1″ a defective enumeration in one axis
of one clause that the new guard's own first sweep skipped. R10's cure is small
and mechanical.
