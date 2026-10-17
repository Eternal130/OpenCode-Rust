# SPEC-R9 — Integration round (MG-3 cure · list-sweep · residuals) — scan-spec **v8**

- **Input**: docs/scan-spec.md **v7** (1048 lines; appendix 140/140; FINAL
  candidate 2) + docs/ratify-1b.md (RATIFY-1′ FAIL — MG-3 material; R-E/R-F/
  M-1..M3 non-blocking).
- **Lane**: `lane/spec9` (worktree `.worktrees/lane-spec9`, branch off main).
  Standing rules honored: foreground only, ≤45-min chunks, English, verbatim
  discipline + gap-ID provenance, no background tasks.
- **Deliverables**: A8 rewritten at census-pinned family granularity; I10b
  **list-census guard** (the process cure); full list-sweep of every
  list-bearing clause (11 findings, L1-L11); residual riders R-E/R-F/M-1..M3;
  appendix +17 rows → **157/157**; v8 declared **FINAL candidate 3**.
- All census commands in this doc were EXECUTED against
  `/opt/opencode-ref/packages/` (2.0.24 source tree) on 2026-10-17; ours-side
  greps against `/opt/opencode-rust/crates/`.

---

## 1 — The censuses (MG-3 ground truth, measured this round)

### 1a. merman — COMPLETE (every kind, every sub-family)

**Census commands** (mechanical, reproducible):

- `sed -n '1p' /opt/opencode-ref/packages/merman/src/diagnostics.ts` →
  `export type MermaidDiagramKind = "flowchart" | "sequence" | "state" |
  "timeline" | "gitGraph" | "gantt"` — **exactly six kinds**.
- `ls -1 /opt/opencode-ref/packages/merman/src/` → core, detect.ts,
  diagnostics.ts, flowchart, gantt, gitgraph, layout-audit.test.ts,
  markdown.ts, palette.test.ts, palette.ts, plugin.ts, sequence, state,
  test, timeline — six kind dirs + shared infra, nothing else.
- Per kind: `ls /opt/opencode-ref/packages/merman/src/<kind>/`.

**Kind x renderer status** (all six are FULL renderers — none a stub; every
kind dir carries parser + drawing + render-grid + style + tests):

| Kind | Files | Lines | Beyond the common shape |
|---|---|---|---|
| flowchart | 12 | 6806 | labels, layout, routing, options |
| sequence | 10 | 2422 | placement |
| state | 18 | 6239 | endpoint, note, layout, routing, search, visible-model |
| timeline | 7 | 508 | — |
| gitGraph | 7 | 734 | — |
| gantt | 7 | 581 | — |

Shared `core/`: 15 files / 1915 lines (canvas incl. `DiagramCanvasSizeError`,
drawing, geometry, spatial, text). `detect.ts` dispatches exactly the six
kinds (is-Mermaid-X-Diagram guards per kind).

**Sub-families BEYOND diagrams** (4, all census-verified):

1. `markdown.ts` (336 lines) — the ```mermaid FENCE renderer: streaming
   last-good cache keyed by block id; on `MermaidSyntaxError` retries the
   partial prefix (line-number cut) and falls back silently when mid-stream;
   `DiagramCanvasSizeError` → silent pass-through (undefined).
2. `palette.ts` — `resolveOpenCodeDiagramPalette(theme, themeMode)` — the
   theme x renderer interaction surface (A8's syntax-theme axis made
   concrete).
3. `plugin.ts` — builtin plugin id `opencode.merman`, registers the
   "mermaid" code-block renderer (wired in `tui/src/plugin/builtins.ts:13`;
   demo'd by storybook `merman-layouts.tsx`, which itself exercises
   `stateDiagram-v2` kinds — state was a first-class demo kind all along).
4. layout-audit (`script/layout-audit.ts` + `src/test/layout-audit/`).

Package exports: `./markdown`, `./palette`, `./plugin`. Consumer: tui only.

### 1b. latex — COMPLETE (and the "inline vs block" premise is WRONG for it)

**Census commands**:

- `grep -n 'registerCodeBlockRenderer' /opt/opencode-ref/packages/latex/src/plugin.ts`
  → TWO registrations, ONE renderer: fences **"latex"** and **"math"**.
- `ls -1 /opt/opencode-ref/packages/latex/src/` → 14 files (8 source, 6 test),
  2646 lines: parser (+`expandLatexMacros`), layout, symbols, limits,
  render, markdown, plugin, types + tests.
- `grep -n 'DEFAULT_MAX_' /opt/opencode-ref/packages/latex/src/limits.ts` →
  source 100_000 chars, nesting depth 256.

**FENCED code blocks ONLY — there is NO inline math on the TUI stack**
(`grep -rn inline packages/latex/src` = 0). The streaming behavior:
lastGood retention gated on `token.text.startsWith(previous.source)` +
fence-streaming detection; parse failure → fallback to the DEFAULT code
renderable. Palette = theme text/subdued. Consumers: `tui/src/plugin/
builtins.ts:12` + `cli/dev/tui.ts`.

### 1c. Where "inline vs block" ACTUALLY lives — session-ui KaTeX

`grep -n 'inlineKatex\|blockKatex' /opt/opencode-ref/packages/ui/src/context/
marked-parser.tsx` → marked extension on KaTeX **0.16.47** (ui/package.json:
117): **inline** `\(...\)` (inlineKatex, displayMode:false) and **block**
`$$\n...\n$$` (blockKatex, displayMode:true); same file binds
`markedShiki` (the session-ui code-syntax leg). A8's v4-v7 phrasing
"math/latex (inline vs block)" conflated the two stacks — cured in the A8
rewrite (both stacks enumerated).

### 1d. Code-block-renderer universe + ours

- `grep -rn 'registerCodeBlockRenderer' /opt/opencode-ref/packages/` →
  exactly TWO builtin registrations (merman "mermaid"; latex "latex"+"math");
  the plugin API surface itself is `plugin/src/tui/context.ts:555` —
  third-party renderers are A5/C6-owned.
- Ours: `grep -ri 'mermaid\|merman\|latex\|katex' /opt/opencode-rust/
  crates/ --include=*.rs` = **0 hits** (2026-10-17) — every renderer cell
  absent-justified, exactly as v7 adjudicated; no verdict change.

### 1e. MG-3 calibration (unchanged from ratify-1b)

Zero current-verdict impact (renderer wholly absent-justified on our side),
but the v7 letter was contradicted by ref source — the R2→v7 verbatim-carry
class that MG-2/census discipline exists to kill. Cured below + guarded
against recurrence (I10b).

---

## 2 — The list-sweep (every list-bearing clause in v7, adjudicated)

Sweep rule: a clause is list-bearing if its letter carries a brace/list
enumeration. Disposition classes: **CENSUSED** (v7 already cited command or
line-exact evidence) · **VALID→PINNED** (census executed this round, list
exact, command now cited) · **CURED** (census contradicted the carried list)
· **NAMED-HONEST** (no static enumeration exists / data-driven; named as
such) · **EXEMPT-BY-CLASS** (authored sweep parameters enumerating OUR test
design, not ref reality — guard-exempt).

| Clause | List carried | Disposition | Action |
|---|---|---|---|
| I10b | 35-package pin | CENSUSED | none (v7 command re-verified) |
| I5 | standing instruments | CENSUSED | none (ratify-1b file-by-file) |
| C22 | OTEL_* env-read set | CENSUSED | none (ratify-1b line-exact) |
| F2/I10b | standing accounting entries | CENSUSED | none (+R-E additions below) |
| **A8** | mermaid kinds; math stacks | **CURED** | rewrite (§3.1) |
| A5 | feature-plugin slots | VALID→PINNED | census command added (R9-L2) |
| A6 | mini panel enum | VALID→PINNED | census command added (R9-L1) |
| A9 | image format set | EXEMPT-BY-CLASS | corpus parameter (formats planted by the harness), not a ref enumeration |
| B6 | terminal rows x cols | EXEMPT-BY-CLASS | designed test matrix; env dispositions measured 2026-10-17 |
| B7 | env-cell set | EXEMPT-BY-CLASS | designed matrix (OTEL_* cells R8-censused; M-2 consistency fix below) |
| B8 | codepoint classes | EXEMPT-BY-CLASS | designed corpus classes |
| B9 | remoting rows | EXEMPT-BY-CLASS | designed matrix |
| C3 | permission decision/persistence sets | VALID→PINNED | ref literals pinned (R9-L3) |
| C9 | MCP verbs | EXEMPT-BY-CLASS | designed lifecycle matrix (+M-3 threshold) |
| **C8** | worktree op set | **CURED** | over-enumeration pinned (R9-L4) |
| C11 | updater methods | VALID→PINNED | updater.ts:14 exact (R9-L5) |
| C13 | skill sources/phases | EXEMPT-BY-CLASS | designed matrix |
| C15 | composer triggers/tabs | EXEMPT-BY-CLASS | designed matrix |
| C17 | PTY verbs | EXEMPT-BY-CLASS | designed lifecycle matrix |
| C16 | dialog trees | VALID→PINNED | census commands named (R9-L6) |
| C18 | CLI handlers | VALID→PINNED | census command named (R9-L7) |
| C19 | integration providers | NAMED-HONEST | runtime `integration.list`, no static enum (R9-L8) |
| C20 | named tool list | VALID→PINNED | enumeration dirs named (R9-L9) |
| C22(3) | OTLP protocol details | CENSUSED | none |
| D6 | desktop/app subpackages | VALID→PINNED | dir census added (R9-L10) |
| D9 | variant kinds | NAMED-HONEST | catalog-data (models-dev per I10) (R9-L11) |
| E1/E3/E4/E6/E7/E12 | cells/shapes/classes/corpora | EXEMPT-BY-CLASS | authored sweep parameters |
| F6 | seed pairs | EXEMPT-BY-CLASS | authored seeds |

Sweep totals: **4 CENSUSED · 9 VALID→PINNED · 2 CURED (A8, C8) · 2
NAMED-HONEST (C19, D9) · the rest EXEMPT-BY-CLASS** — no uncensused
ref-enumerating list survives in v8.

---

## 3 — The cures (all carried VERBATIM into v8; quotes are character-exact
extracts of docs/scan-spec.md v8)

### 3.1 A8 rewrite (MG-3 cure — 51 lines in spec)

````
A8 **Rich content** (v4 rewrite per R3 §2/§5 + R2 §c restored; **v8 rewrite
   per R9 — MG-3 cure: every family/sub-family list below is census-pinned
   against the ref tree, mechanical, not prose**): [G] per content family —
   markdown, code-syntax, mermaid/merman, math/latex (BOTH renderer stacks,
   below), diff, part-kind, `thinking-syntax.ts` (R3 fold 3; exists at
   `tui/src/routes/session/thinking-syntax.ts`) — x size x **syntax-theme
   (theme x renderer interaction)**. **Mermaid sub-families (census-pinned
   v8):** the ref kind union is EXACTLY {flowchart, sequence, state,
   timeline, gitGraph, gantt} — six kinds, ground truth
   `merman/src/diagnostics.ts:1` (`MermaidDiagramKind`); `detect.ts`
   dispatches exactly these six; census commands: `sed -n '1p'
   /opt/opencode-ref/packages/merman/src/diagnostics.ts` and `ls -1
   /opt/opencode-ref/packages/merman/src/`. **Renderer status: ALL SIX are
   FULL renderers, none a stub** — each kind dir carries parser + drawing +
   render-grid + style (+ tests): flowchart 12 files (adds
   labels/layout/routing/options), sequence 10 (adds placement), state 18
   (adds endpoint/note/layout/routing/search/visible-model), timeline 7,
   gitGraph 7, gantt 7; shared `core/` 15 files (canvas/drawing/geometry/
   spatial/text). **Beyond diagrams, merman's census closes 4 more
   sub-families:** `markdown.ts` (the ```mermaid fence renderer: streaming
   last-good cache + partial-prefix re-render on `MermaidSyntaxError`,
   silent pass-through on `DiagramCanvasSizeError`), `palette.ts`
   (`resolveOpenCodeDiagramPalette(theme, themeMode)` — THE theme-x-
   renderer interaction cell), `plugin.ts` (builtin `opencode.merman`,
   registered in `tui/src/plugin/builtins.ts`), layout-audit (`script/` +
   tests). **Math/latex = TWO renderer stacks (census-pinned v8; the
   v4 "inline vs block" phrasing mis-attributed the split):** (a) TUI
   stack = `@opencode/latex` — FENCED code blocks ONLY, fence languages
   EXACTLY {latex, math} (census: `grep -n 'registerCodeBlockRenderer'
   /opt/opencode-ref/packages/latex/src/plugin.ts` — two registrations,
   one renderer); sub-parts: parser (+`expandLatexMacros`), layout,
   symbols tables (`symbolTable`/`largeOperators`/`namedOperators`/
   `delimiterTable`/`spacingCommands`), limits (100k source chars / 256
   nesting), streaming retention + code-fallback on parse failure; NO
   inline math exists on the TUI stack. (b) session-ui stack = KaTeX
   0.16.47 via marked extension (`ui/src/context/marked-parser.tsx`):
   inline `\(...\)` (inlineKatex, displayMode:false) + block
   `$$\n...\n$$` (blockKatex, displayMode:true) — census: `grep -n
   'inlineKatex\|blockKatex' .../marked-parser.tsx`; the inline-vs-block
   axis lives HERE (plus markedShiki = the session-ui code-syntax leg).
   **Code-block renderer universe census:** `grep -rn
   'registerCodeBlockRenderer' /opt/opencode-ref/packages/` → exactly
   two builtin registrations (merman "mermaid"; latex "latex"+"math") —
   plugin-registered renderers beyond these are A5/C6-owned. **Ours:
   absent-justified per renderer** — `grep -ri 'mermaid\|merman\|latex\|
   katex' crates/` = 0 code hits (2026-10-17): all 6 mermaid kinds, the
   merman fence/palette pipeline, the TUI latex fences, and the
   session-ui KaTeX stack are ref-only; each cell = absent-justified
   accounting until dispositioned. [F] I1 + **I9 corpus provenance**
   [M] renderer corpus diff — rendered frames, not parse trees; render
   parity per family. {R2: S1-4, (a)S1-4; R3: §2-A8, FOLD-3; R9: MG-3}
````

### 3.2 I10b list-census guard (the process cure)

````
**List-census guard (v8 — R9-MG3; extends this pin's
    discipline from packages to EVERY in-clause list):** any clause that
    CARRIES a list purporting to enumerate ref reality (diagram kinds,
    fence languages, panel/component files, tool names, handler files,
    updater methods, env-read sets, status/state unions, package names,
    …) must cite the MECHANICAL census command that reproduces it
    (`ls`/`grep`/`sed` against the pinned ref tree, or the I2b binary/
    its APIs where no static enumeration exists) — exactly as this pin
    does for packages; lists that enumerate OUR test design (sizes,
    counts, locale/query classes, seed pairs, sweep widths) are exempt
    by class. A carried list without a census citation is the MG-3
    defect class (prose recall contradicting ref source) and FAILS
    ratification on sight. The v8 list-sweep (docs/spec-round9.md §2)
    adjudicated every list-bearing clause in this file: censused-and-
    valid, cured-and-pinned, data-driven-named, runtime-named, or
    exempt-by-class. {R9: MG-3}
````

### 3.3 I10b standing entries — R-E (function + web)

````
Standing accounting entries, no product-side client
    (v8, R-E): **function** (Cloudflare-worker share backend: `/share_create`
    `/share_sync` `/share_poll` `/share_delete[_admin]`, SyncServer
    DurableObject, R2, GitHub-App auth — hosted server-side; no client in
    the pinned 2.0.24 source NOR the 2.0.25 binary, verified 2026-10-17 in
    docs/ratify-1b.md: the `stats`-class; accounting entry only — the share
    product family is D8-owned, a future product client re-baselines per
    I10); **web** (share viewer polling `ws /share_poll?id=` + docs site —
    viewer-side of the same hosted pair; rides D8's destination cells;
    accounting entry only). {R3: U8, U9, U12; R4: re-sweep; R9: R-E}
````

### 3.4 I5 extension — R-F (mock OTLP collector)

````
**R9 extension (v8 — R-F):** the list additionally
   binds C22's mock OTLP collector (loopback HTTP recorder asserting
   /v1/logs + /v1/traces POSTs and zero-traffic-when-unset) —
   clause-declared standing since v7, formally I5-named from v8; the
   first C22 execution commits it in-tree. {R9: R-F}
````

### 3.5 Census pins + riders (L1-L11, M-2, M-3)

**A4 clock note (M-3):**

````
**V8 clock note (R9-M-3):** the vt-clock drives OUR leg; the ref leg is
   captured at pinned REAL cadences (ref's process is not vt-clocked) —
   stepped-frame comparison happens at the same named cadence sources.
   {R1: S4-4; R9: M-3}
````

**A5 pin (L2):**

````
**V8 census pin:** slot list verified against `ls -1
   /opt/opencode-ref/packages/tui/src/feature-plugins/` = exactly {home,
   prompt, sidebar, system} (2026-10-17). {R1: S1-11, S3-4; R2: (a)S1-11;
   R3: FOLD-5; R9: L2}
````

**A6 pin (L1):**

````
**V8 census pin:** panel files verified against `ls -1
   /opt/opencode-ref/packages/tui/src/mini/ | grep -E
   '^(footer|scrollback|stream-v2|splash)'` — all 12 listed panels exist as
   files; `stream-v2.transport.ts` is plumbing, not a panel (2026-10-17).
   {R1: S1-5; R2: (a)S1-5; R9: L1}
````

**B7 LIVE-list addition (M-2):**

````
OTEL_* (the R8 cells
   below; v8 consistency addition, R9 M-2)
````

**C3 pin (L3):**

````
**V8 census pin (R9-L3):** ref's literal
   sets = Permission.Effect {allow, deny, ask} (`schema/src/permission.ts:55`)
   and Permission.Reply {once, always, reject} (`:41`) — the decision x
   persistence matrix materializes over THESE literals (allow/deny/always
   above are the behavioral buttons they compose into); census: `grep -n
   'Schema.Literals' /opt/opencode-ref/packages/schema/src/permission.ts`.
   [F] I1+I3+real grant stores [M] flow
   scripts + emission capture trace, scripted flows end-to-end.
   {R1: S1-8, S5-6; R2: (a)S5-6; R9: L3}
````

**C7 pin (M-3):**

````
**V8 census pin (R9-M-3):** the job-state enumeration source is the
   tool-part status union = {streaming, running, completed, error} —
   `schema/src/session-message.ts:127/:133/:140/:148` (C1's from-ref-code-
   not-prose discipline); census: `grep -n 'status: Schema.tag'
   /opt/opencode-ref/packages/schema/src/session-message.ts`. {R9: M-3}
````

**C8 cure (L4):**

````
**V8 census cure (R9-L4):** ref's ACTUAL worktree op set =
   {create, remove, refresh, list} (`grep -n 'Effect.fn("Worktree\.'
   /opt/opencode-ref/packages/core/src/worktree.ts` — exactly four; +
   strategy registry, default git) plus the `session_move` tool
   (`core/src/tool/plugin/opencode.ts:110` — the "move" axis) and the
   create-time name dialog (`dialog-worktree-name.tsx`); **`adopt` and
   `rename` have NO ref op** (the R2 list over-enumerated — carried
   R2→v7 uncensused; adopt/rename cells convert to ours-extra negative-
   space rows under F7, not parity cells). {R1: S1-1; R2: (a)S1-1; R9: L4}
````

**C9 threshold note (M-3):**

````
**V8 threshold note (R9-M-3):** threshold = every cell
   completes its round-trip with matching observable state on both stacks
   (implicit in the verb, now stated). {R1: S1-2; R2: (a)S1-2; R9: M-3}
````

**C11 pin (L5):**

````
**V8 census pin (R9-L5):** the method
   list is EXACT — `cli/src/services/updater.ts:14` = `["curl", "npm",
   "pnpm", "bun", "yarn", "vp", "brew"]`; census: `sed -n '14p'
   /opt/opencode-ref/packages/cli/src/services/updater.ts`. [F] real updater paths (staged update)
   [M] upgrade round-trip script per method; UX + fs assert.
   {R1: S1-4, S2-9; R2: (a)S1-4; R9: L5}
````

**C16 pin (L6):**

````
**V8 census pin (R9-L6):**
   census commands = `ls -d /opt/opencode-ref/packages/tui/src/component/
   dialog-*` (23 dirs) ∪ `ls -d /opt/opencode-ref/packages/tui/src/routes/
   session/dialog-*` (4) ∪ session-ui components (counted 2026-10-17; the
   three trees are the enumeration, the named list above is illustrative
   cells within them). {R2: S1-8, (a)C16; R3: §2-C16, FOLD-6; R9: L6}
````

**C18 pin (L7):**

````
**V8 census pin (R9-L7):** census = `ls -1R
   /opt/opencode-ref/packages/cli/src/commands/handlers/` (top-level:
   acp, api, auth, debug, default, mini, models, pair, plugin, reload,
   run, serve, service, session/, mcp/, stats, uninstall, upgrade —
   counted 2026-10-17; nested dirs enumerate recursively).
   {R3: U4, U5; R9: L7}
````

**C19 note (L8):**

````
**V8 census note (R9-L8): NO static enumeration exists** —
   `IntegrationID` is a branded string (`schema/src/integration-id.ts:3`);
   providers are runtime/plugin-registered and enumerate only via the
   `integration.list` API (consumed by `dialog-integration.tsx:85`); census
   = the list-API result on the I2b binary leg, recorded per round with
   which-ref-form per I2b. [F] I1 [M] flow assert + credential store check.
   {R3: U6; R9: L8}
````

**C20 pin (L9):**

````
**V8 census pin (R9-L9):** the tool universe enumerates from TWO dirs —
   `ls -1 /opt/opencode-ref/packages/core/src/tool/` (read-filesystem, mcp,
   html-markdown, http-body, runtime, patch.txt) ∪ `ls -1
   /opt/opencode-ref/packages/core/src/tool/plugin/` (edit, file-diff,
   glob, grep, mcp-resource, opencode, patch, question, read, shell,
   skill, subagent, webfetch, websearch, write — counted 2026-10-17; the
   named list above is illustrative, the [G] covers every census entry).
   [F] I1+I2 [M] schema+behavior+render parity vs ref per tool.
   {R3: U7; R9: L9}
````

**D6 pin (L10):**

````
**V8 census pin (R9-L10):** `ls -1
   /opt/opencode-ref/packages/desktop/src/` = exactly {main, preload,
   renderer, shared}; `ls -1 /opt/opencode-ref/packages/app/src/` =
   {app.tsx, composer, desktop.ts, entry.tsx, home, new-session, providers,
   runtime, servers, session, settings, shell, workspaces, …} — the clause's
   named subpackages all exist (counted 2026-10-17).
   {R1: S1-7, S3-7; R2: (a)S1-7; R3: U10; R9: L10}
````

**D9 note (L11):**

````
**V8 census note (R9-L11):** the variant-kind set
   {effort, toggle, budget_tokens} is CATALOG-DATA — it lives in models-dev
   data (I10-pinned external snapshot), not in code (`schema/src/model.ts`
   defines Variant = {id + settings/headers/body overlays}, untyped kinds);
   census = the pinned models-dev snapshot, re-run on I10 re-baseline.
   [F] catalog + config variants + I10 [M] chain assert
   (config -> catalog -> dialog -> request). {R1: S3-6; R2: S1-7, (a)D9; R3: §2-D9;
   R9: L11}
````

### 3.6 M-1 (appendix letter fix)

R8-N6's appendix status letter corrected in place: **A → REG**, with the
correction note recorded in the row itself (a decision record, not an
amendment; the structural cure lives in R8-MG1's row/destination C22). No
clause text touched; zero-loss property unaffected.

---

## 4 — Zero-loss re-check (measured, not trusted)

- **Appendix arithmetic (row-counted)**: R1 52 · R2 30 · R3 20 · R5 12 ·
  R7 15 · R8 11 · **R9 17** = **157/157**; header claim and legend updated
  to match; round-log chain 102 → 114 → 129 → 140 → 157 consistent.
- **Clause universe unchanged**: 83 clauses (A1-A9, B1-B9, C1-C22, D1-D11,
  E1-E12, F1-F7, I1-I11 incl. I2b/I10b) — R9 added NO new clause and
  renumbered NOTHING (A8 rewritten in place; all other cures are riders/
  pins inside existing clauses).
- **Bidirectionality**: every R9-# tag in the appendix maps to a clause
  rider that carries the same tag (grep-verified: R9-MG3→A8+I10b, L1→A6,
  L2→A5, L3→C3, L4→C8, L5→C11, L6→C16, L7→C18, L8→C19, L9→C20, L10→D6,
  L11→D9, R-E→I10b, R-F→I5, M-1→appendix R8-N6 row, M-2→B7, M-3→C7+C9+A4).
- **Verbatim carry**: every quoted cure block in §3 is a character-exact
  extract of the v8 spec text (extracted programmatically from the file,
  not re-typed).
- **No stale claims introduced**: the v7 texts that the cures replace (A8's
  4-kind list, "inline vs block" for latex, C8's adopt/rename cells, B7's
  LIVE list gap, N6's "A" letter) are superseded by the v8 texts in the
  same locations; no second copy of any replaced list exists (grep-verified:
  'flowchart/gantt/gitgraph/sequence' = 0 hits in v8; every remaining
  'adopt' / 'inline vs block' occurrence is corrective narrative — the
  retained R2 verbatim matrix in C8 + its same-clause census cure, the
  A8/appendix correction records — none is a live enumeration premise).
- **v7 growth damaged nothing**: all v6/v7 riders and texts referenced by
  ratify-1b re-verified present (E6/E9/C7/B3 R8 riders, C22, I10b census
  command, I2b, E2 legs — spot-checked by grep during integration).

---

## 5 — Declaration

**docs/scan-spec.md v8 = FINAL candidate 3.** MG-3 is cured at true family
granularity (census-pinned, [G][F][M] complete); the list-census guard
makes the defect class structurally non-recurring; the R2→v7 carry class is
extinct (no uncensused ref-enumerating list survives); R-E/R-F/M-1..M3 are
dispositioned. Appendix 157/157; clause universe 83; zero-loss verified.

Per docs/scan-program.md: the ratification counter RESETS — **RATIFY-1″ and
RATIFY-2″ follow on FINAL candidate 3** (fresh stances, fresh samples, plus
regression checks on the R9 cures: the A8 rewrite's census claims, the
guard's sweep-table completeness, and the C8 over-enumeration cure's
negative-space disposition).
