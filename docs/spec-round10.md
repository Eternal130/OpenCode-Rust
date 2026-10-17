# SPEC-R10 — Integration round (MG-4 cure · strengthened guard · exhaustive sweep · M-4 · R-G1) — scan-spec **v9**

- **Input**: docs/scan-spec.md **v8** (1246 lines; appendix 157/157; FINAL
  candidate 3) + docs/ratify-1c.md (RATIFY-1″ FAIL — MG-4 material; R-G1 and
  M-4/M-5 non-blocking).
- **Lane**: `lane/spec10` (worktree `.worktrees/lane-spec10`, branch off main).
  Standing rules honored: foreground only, ≤45-min chunks, English, verbatim
  discipline + gap-ID provenance, no background tasks.
- **Deliverables**: C14(b) source-enum cure (MG-4); **enumeration-census
  guard STRENGTHENED** (unions/kinds/sources/named-cell inventories/count
  annotations; count-vs-list coherence; mandatory exhaustive sweep) +
  **R10 exhaustive sweep — 83/83 clauses** (§3, discovery procedure stated,
  re-runnable); numeric-annotation audit (§4); A6 numeral fix (M-4); five
  R-G1 standing accounting entries; C13 census pin + C20/I10b nano-cures
  (M-5); appendix +5 rows → **162/162**; v9 declared **FINAL candidate 4**.
- All census commands in this doc were EXECUTED against
  `/opt/opencode-ref/packages/` (2.0.24 source tree) on **2026-10-18**
  (R9's 2026-10-17 results re-executed, not trusted); ours-side greps
  against `/opt/opencode-rust/crates/`.

---

## 1 — MG-4 ground truth: ref's ACTUAL instruction-discovery source universe (measured this round)

### 1a. What discovers what (the census)

| Source kind | Discovery mechanism | Census evidence |
|---|---|---|
| **Commands** | glob `{command,commands}/**/*.md` | `grep -n 'command,commands' …/core/src/config/plugin/command.ts` → `:146` comment + `:159` `fs.scan("{command,commands}/**/*.md", …)` |
| **Agents** | globs `{agent,agents}/**/*.md` (secondary) ∪ `{mode,modes}/*.md` (**primary**) | `sed -n '22,23p' …/core/src/config/plugin/agent.ts` → `legacySources = [{ pattern: "{agent,agents}/**/*.md", primary: false }, { pattern: "{mode,modes}/*.md", primary: true }]` |
| **Instructions** | `AGENTS.md` ONLY — global-config file (:38), project dirs (:48), ancestor walk `fs.up({targets: ["AGENTS.md"]})` (:72) | `grep -n 'AGENTS\.md' …/core/src/config/plugin/instruction.ts` |
| **Skills** | `{directory, url, embedded}` sources | `grep -n 'type Source' …/schema/src/skill.ts` → `:45` `DirectorySource \| UrlSource \| EmbeddedSource` (paths/urls wired at `config/normalize.ts:243-244`; embedded arrives via plugins) |
| **Prompts** | **NO markdown/discovery source. NONE.** | `ls -1 …/core/src/config/plugin/` → agent, command, compaction, compatibility, entry-observer, formatter, image, instruction, location-watcher, mcp, policy, provider, reference, shell, skill, skill-file, snapshot, source, tool-output, websearch, worktree — **no prompt source plugin**; `grep -n 'Prompt' …/config/plugin/{command,agent}.ts` = **0 hits** |

### 1b. Where "prompt" actually lives in ref (and why it is not a source kind)

- **MCP prompts are runtime-served, not file-discovered**:
  `core/src/plugin/command.ts:17-27` — `loaded = { prompts: [] as
  Mcp.Prompt[] }`, populated from `mcp.prompts()` (server-provided),
  reloaded on `Mcp.PromptsChanged` events. This is a C9/C20-class runtime
  surface (MCP family + tool invocation), exactly as ratify-1c proposed.
- The plugin API registers **agent + command collections ONLY**
  (`plugin/src/tui/context.ts:128-129`: `readonly agent:
  LocationCollection<AgentInfo>`, `readonly command:
  LocationCollection<CommandInfo>`); every other "prompt" symbol in that
  file is the `prompt.footer.*` composer slots (:184-:197) and the
  `prompt(options)` ask-dialog (:403) — a different family.
- **Decoy pre-empted**: `core/src/plugin/command/initialize.txt:14` names
  "instruction files (`AGENTS.md`, `CLAUDE.md`, `.cursor/rules/`,
  `.cursorrules`, `.github/copilot-instructions.md`)" — this is
  **guided-setup PROMPT COPY** (what the init command's LLM prompt tells
  the model to read when writing an AGENTS.md), NOT discovery code:
  `grep -rn 'CLAUDE\.md\|cursorrules' /opt/opencode-ref/packages/core/src/config/` =
  **0 hits**. A future ratifier grepping for these names will find only
  prompt copy; instruction discovery reads `AGENTS.md` only (§1a).

### 1c. The defect, confirmed

C14(b)'s [G] axis "source enum {command, agent, prompt}" is the R2 S1-2
wording carried verbatim R2→v8 (scan-spec v8 :535-537), never censused.
`prompt` over-enumerates (no discovery source); `mode`/`modes`
UNDER-enumerates on the agent axis (never named); the [F] leg
(`.opencode/{command,agent}/*.md`) was already the real shape — the [G]
contradicted the clause's own [F]. Same register as C8's {adopt, rename}
(R9-L4) and MG-3's 4-of-6: an R2-carried uncensused enum. The v8
list-sweep missed it because R9's sweep table adjudicated a curated 26+
clause set, not the 83-clause universe — the guard's letter held, its
first execution didn't. Cured in-clause (§2.1) + structurally (§2.2).

---

## 2 — The cures (all carried VERBATIM into v9; quotes are character-exact extracts of docs/scan-spec.md v9)

### 2.1 C14(b) census cure + (a) census note (MG-4) — C8-pattern: R2 verbatim [G] retained, same-clause cure appended

````
   **V9 census cure (R10-MG4):** the (b) source-enum axis is
   OVER-ENUMERATED as carried — re-dispositioned to ref's TRUE discovery
   census: **commands = `{command,commands}/**/*.md`**
   (`core/src/config/plugin/command.ts:159`); **agents =
   `{agent,agents}/**/*.md` ∪ `{mode,modes}/*.md`** — modes are PRIMARY
   agents (`core/src/config/plugin/agent.ts:22-23`); **`prompt` has NO
   markdown/discovery source in ref** — census: `ls -1
   /opt/opencode-ref/packages/core/src/config/plugin/` (command, agent,
   skill, skill-file, instruction, … — NO prompt source plugin) and
   `grep -n 'Prompt' …/config/plugin/{command,agent}.ts` = 0 hits; the
   enum axis materializes over **{command, agent} ONLY** (the [F] leg's
   own `.opencode/{command,agent}/*.md` was already the real shape).
   Prompts ARE real in ref but **RUNTIME-SERVED, not file-discovered**:
   MCP prompts (`Mcp.Prompt[]` via `mcp.prompts()`, reloaded on
   `Mcp.PromptsChanged` — `core/src/plugin/command.ts:17-27`,
   server-provided) → C9/C20-owned cells; an OURS-side prompt source,
   if ever shipped, is an F7 ours-extra row (the C8 adopt/rename rule).
   The plugin API registers agent + command collections ONLY
   (`plugin/src/tui/context.ts:128-129`; its "prompt" symbols are the
   prompt.footer composer slots + the `prompt()` ask-dialog — a
   different family). **(a) census note (same cure):** instruction
   discovery targets **`AGENTS.md` ONLY** (`config/plugin/instruction.ts`
   — global-config file :38, project dirs :48, ancestor walk
   `fs.up({targets: ["AGENTS.md"]})` :72); the CLAUDE.md /
   .cursorrules / .cursor/rules / .github/copilot-instructions.md names
   in `plugin/command/initialize.txt:14` are guided-setup PROMPT COPY
   (what the init command tells the model to read), NOT discovery code
   (`grep -rn 'CLAUDE\.md\|cursorrules' packages/core/src/config/` =
   0). Census commands: `grep -n 'command,commands'
   …/core/src/config/plugin/command.ts`; `sed -n '22,23p'
   …/core/src/config/plugin/agent.ts`; `grep -n 'AGENTS\.md'
   …/core/src/config/plugin/instruction.ts` (2026-10-18).
   {R2: S1-2, §c-C14; R3: §2-C14; R10: MG-4}
````

### 2.2 I10b guard STRENGTHENED (the process cure for the sweep-completeness failure)

````
    **Enumeration-census guard, STRENGTHENED
    (v9 — R10; closes the MG-4 execution gap):** the guard's unit is
    EVERY in-clause enumeration of ref facts, in ANY syntactic form —
    brace lists, union/kind/source ENUMS (the form that escaped R9's
    sweep: C14(b)'s {command, agent, prompt}), named-file inventories
    ("named cells"), and the COUNT annotations and numerals that
    summarize an enumerated set ("all 12 listed panels", "6 exhaustive +
    29 family-granular") — each citing the mechanical census command
    that reproduces it; a count annotation must AGREE with the
    enumeration it summarizes (a disagreeing numeral inside a census
    pin is the M-4 defect class and fails on sight). A sweep enforcing
    this guard is valid ONLY if it enumerates the FULL 83-clause
    universe row-by-row (no-enumeration clauses included, marked as
    such) via a stated mechanical discovery procedure the next ratifier
    can re-run — a curated-subset sweep is itself a fail-on-sight
    defect (the MG-4 lesson: C14(b) escaped R9's 26+-clause curated
    sweep while the guard's letter claimed completeness). The R10 sweep
    (docs/spec-round10.md §3) is the first execution under this rule:
    83/83 rows, discovery procedure stated. {R10: MG-4, SW1}
````

### 2.3 A6 numeral fix (M-4)

````
   **V8 census pin:** panel files verified against `ls -1
   /opt/opencode-ref/packages/tui/src/mini/ | grep -E
   '^(footer|scrollback|stream-v2|splash)'` — all 13 listed panels exist as
   files; the grep yields 17 files = the 13 panels + 4 plumbing/shared
   (`footer.ts`, `footer.width.ts`, `scrollback.shared.ts`,
   `stream-v2.transport.ts` — plumbing, not panels) (2026-10-17).
   **Numeral corrected v9 (R10-M-4):** the v8 pin said "all 12 listed
   panels" — the clause's own enum lists 13 (footer x7, scrollback x2,
   splash x1, stream-v2 x3) and all 13 exist; count-annotation
   coherence is guard-enforced from v9.
   {R1: S1-5; R2: (a)S1-5; R9: L1; R10: M-4}
````

### 2.4 R-G1 class-cure — five standing accounting entries (I10b)

````
    Standing accounting entries, no parity surface (v9, R-G1 — the
    class-cure: ratify-1.md's table adjudications made standing, same
    format as console/posts/script/function/web; census commands =
    `find /opt/opencode-ref/packages/<pkg> -type f` each, executed
    2026-10-18): **containers** (8 files: 5 Dockerfiles {base, bun-node,
    rust, tauri-linux, publish} + `script/build.ts` + README/tsconfig —
    build/CI recipes, no runtime surface; rides B7/B9 docker cells),
    **storybook** (31 files: `.storybook/` config {main, manager,
    preview, extension, theme-tool} + `mocks/{app,gui-extensions}` —
    harness/mocks, no src/ product code), **effect-drizzle-sqlite**
    (2 files: `sst-env.d.ts` + one example migration.sql under
    `examples/` — no src/, no package.json; the drizzle adapter it
    examples lives in `core/src/database/drizzle.ts` (core-owned)),
    **httpapi-codegen** (16 files: build-time code generator — its
    OUTPUT is shipped (`client/src/effect/generated/*`, header
    "Generated by @opencode/httpapi-codegen") and that output's parity
    is D11-owned via the client shape-diff; the generator itself is
    dev tooling), **http-recorder** (30 files: record-once/replay
    cassette library for Effect HTTP/WebSocket — test infrastructure;
    consumers = `ai/test/*` + `core/test/*` only, zero runtime importers).
    Identity precision note (v9, M-5b): the U9 OOC entry's
    "account/identity" label is coarser than reality — `identity/` is 6
    brand-mark asset files (mark.svg + PNG variants), zero code
    (`find packages/identity -type f` = 6, all assets); disposition
    (not shipped by us) unchanged. {R10: R-G1, M-5}
````

### 2.5 Sweep nano-cures: C13 census pin (new) + C20 AGENTS.md note (M-5a)

**C13 (VALID→PINNED this round — the source enum was carried uncited; the
strengthened sweep found it and pinned it):**

````
   **V9 census pin (R10-SW1):** the source-kind set
   is EXACT — `schema/src/skill.ts:45` `type Source =
   DirectorySource | UrlSource | EmbeddedSource` (config-side discovery
   wires paths/urls at `config/normalize.ts:243-244`, embedded arrives via
   plugins); census: `grep -n 'type Source'
   /opt/opencode-ref/packages/schema/src/skill.ts` (2026-10-18).
   {R2: S1-1, (a)C13; R3: §2-C13; R10: SW1}
````

**C20 (parenthetical appended inside the v8 pin):** "…the [G] covers every
census entry; the first `ls` also yields `AGENTS.md` — a doc file, not a
tool, the census's only non-tool entry; noted v9, R10-M-5)." (grep-replaced
in place; `{R3: U7; R9: L9}` → `{R3: U7; R9: L9; R10: M-5}`.)

---

## 3 — The R10 EXHAUSTIVE sweep (83/83 clauses, strengthened guard)

### 3.0 Discovery procedure (mechanical, re-runnable by the next ratifier)

1. **Clause universe** (single line, verbatim re-runnable):
   `grep -oE '^(I[0-9]+b?|[A-F][0-9]+) (\*\*|[A-Za-z])' docs/scan-spec.md | wc -l`
   → **83** (13 I incl. I2b/I10b · 9 A · 9 B
   · 22 C · 11 D · 12 E · 7 F). E6's lowercase header ("E6 i18n/locale")
   is why the character class is `[A-Za-z]`, not `[A-Z]` — a naive
   uppercase filter yields 82 and silently drops E6. (Line-wrap hazard:
   a wrapped prose line that BEGINS with a clause-ID + space counts too —
   v8's lineage had one ("A8 rewrite …"); it was re-wrapped in v9 so the
   count is exactly 83. Re-check after any future edit.)
2. **Enumeration extraction**: per clause, every brace group NOT matching
   the gap-ID tag grammar (`\{R[0-9]` / `\{R[0-9]+:` provenance tags),
   every "named:"/"named cells"/"incl." file inventory, and every numeral
   that summarizes an enumerated set (audit in §4). Mechanical aid:
   `grep -nE '\{[A-Za-z][^}]*[,}]' docs/scan-spec.md` minus gap-ID lines,
   then manual per-clause adjudication of intent (ref fact vs authored
   test design) — intent is the one judgment step, and every judgment
   below names its evidence.
3. **Adjudication classes** (R9's five + three the strengthened guard
   adds): **CENSUSED** (in-clause census command; re-executed this round)
   · **VALID→PINNED** (verified true; pin added this round) · **CURED**
   (contradicted; re-dispositioned this round) · **RUNTIME-NAMED** (no
   static enum; API census named) · **DATA-DRIVEN-NAMED** (enumeration
   materializes from a named data source) · **NAMED-CELL-VERIFIED** (file
   inventory; every file existence-checked this round — new) ·
   **VALUE-PINNED** (scalar ref constants with source cites; re-verified
   line-exact — new) · **EXEMPT-BY-CLASS** (authored test design) ·
   **NO-ENUM** (no ref enumeration carried — new; the curated-sweep
   escape hatch, now closed: absence is adjudicated, not skipped).

### 3.1 The table (one row per clause — 83 rows)

| # | Clause | Enumeration(s) carried | Class | Verification executed 2026-10-18 |
|---|---|---|---|---|
| 1 | I1 | — | NO-ENUM | — |
| 2 | I2 | "≥3 ecosystems + omo-slim" | EXEMPT | authored test parameter |
| 3 | I2b | 2.0.24/2.0.25 + module-error facts | NO-ENUM | evidence-cited facts (rescan F10 log id), not enumerations |
| 4 | I3 | spinner 80ms; backoff 1s/5s; StartupLoading 500ms/3s | VALUE-PINNED | `spinner.tsx:21` = 80 · `app.tsx:1313` = 1000/5000 · `startup-loading.tsx:46` = 500, `:22` = 3000 (all line-exact; file = `tui/src/component/startup-loading.tsx`) |
| 5 | I4 | — | NO-ENUM | — |
| 6 | I5 | standing instrument file list | NAMED-CELL-VERIFIED | every path existence-checked in-tree: golden_tests, frame_budget_tests, replay, mouse_modes, diff_viewer_golden_tests (crates/oc-tui/src/…), spec_e1/e5/e2/e12 + e12_search_corpus.json + e3_leak_canary, rescan-tui.rs, tools/parity/spec-p3/ (incl. refbin_tti_results.json), ref packages/simulation — all exist |
| 7 | I6 | umbrella example classes; "41 steps / 2 scripts" | NO-ENUM | illustrative examples; the 41/2 are RESCAN §4 evidence-cited history (I9 provenance) |
| 8 | I7 | — | NO-ENUM | — |
| 9 | I8 | canary designs; matrix status sets | EXEMPT | authored design; statuses are artifact-recorded executions |
| 10 | I9 | — | NO-ENUM | — |
| 11 | I10 | — | NO-ENUM | — |
| 12 | I10b | 35-package pin (6 exhaustive + 29 family-granular); standing entries console/posts/script/devtools/function/web + five R10 | CENSUSED | `ls -1 …/packages/ \| wc -l` = 35; six exhaustive names re-verified {core, tui, cli, session-ui, schema, plugin}; five R-G1 entries: `find <pkg> -type f` = 8/31/2/16/30 files |
| 13 | I11 | — | NO-ENUM | — |
| 14 | A1 | state-set union (10 states) | EXEMPT | authored axis union (ratify-1c ruling: authored-by-class) |
| 15 | A2 | widths {40,60,80,120,160} | EXEMPT | authored sweep widths |
| 16 | A3 | part-kind enum; widths/msgs/wide-char/overflow sets | DATA-DRIVEN-NAMED + EXEMPT | part-kind "materialized from oc-schema, not prose" — the materialization IS the census; the rest are authored sweep parameters |
| 17 | A4 | animated families (spinner, pulse, shimmer, marquee, fade, scrim) | EXEMPT | illustrative families; the named cadences are I3's (VALUE-PINNED) |
| 18 | A5 | slots {home, prompt, sidebar, system} | CENSUSED | pin re-executed: `ls -1 …/tui/src/feature-plugins/` = exactly those 4 |
| 19 | A6 | panel enum (13) + count | CENSUSED | grep re-executed = 17 files = 13 panels + 4 plumbing; numeral FIXED this round (M-4, §2.3) |
| 20 | A7 | theme-chain steps | EXEMPT | authored process decomposition |
| 21 | A8 | mermaid kind union (6); per-kind counts 12/10/18/7/7/7; core 15; 4 sub-families; fence {latex, math}; 2 stacks; renderer universe; ours = 0 | CENSUSED | ALL re-executed exact: diagnostics.ts:1 = 6 kinds; per-kind `ls \| wc -l` = 12/10/18/7/7/7; **core count form: `find merman/src/core -type f \| wc -l` = 15** (`ls -1` shows 14 entries because `color/` is a dir of 2 files — recorded so the next ratifier reproduces 15); markdown.ts/palette.ts/plugin.ts/layout-audit all exist; `grep -c registerCodeBlockRenderer latex/src/plugin.ts` = 2; `grep -rn … packages/` = 2 builtin sites; 5 symbol tables verified at `latex/src/symbols.ts:209/:219/:236/:272/:300`; ours `grep -ri … crates/` = 0 |
| 22 | A9 | origin families + format set | EXEMPT | authored corpus (formats planted by harness — R9 ruling re-affirmed) |
| 23 | B1 | axes only | NO-ENUM | bindings data-driven from ref keymaps at run time |
| 24 | B2 | pointer verbs/families | EXEMPT | authored matrix |
| 25 | B3 | stacks | NO-ENUM | "stack enumeration" is the ARTIFACT (runtime-derived), no carried list |
| 26 | B4 | paste forms {bracketed, image, large, WHATWG} | EXEMPT | authored injection classes |
| 27 | B5 | terminal event families | EXEMPT | authored matrix |
| 28 | B6 | rows x cols matrix; OSC protocol numbers | EXEMPT | authored matrix; OSC 10/11/12/52/777 are protocol constants (test parameters) |
| 29 | B7 | env-cell set; named cells `ssh-askpass.ts`, `editor-zed*`, `attention-sounds.{bun,node}` | EXEMPT + NAMED-CELL-VERIFIED | matrix authored; every named file existence-checked: `cli/src/ssh-askpass.ts`, `tui/src/editor-zed.ts` + `editor-zed-sqlite.{bun,node}.ts`, `tui/src/attention-sounds.{bun,node}.ts` — all exist |
| 30 | B8 | codepoint classes | EXEMPT | authored corpus classes |
| 31 | B9 | remoting rows | EXEMPT | authored matrix; dispositions re-measured by ratify-1c (LIVE tools present) |
| 32 | C1 | named cells dialog-fork/anchors/anchor-view/grouping/{session,tree}/message-navigation | NAMED-CELL-VERIFIED | all existence-checked: `tui/src/routes/session/{dialog-fork.tsx, anchors.ts, anchor-view.tsx, message-navigation.ts}` + `grouping/` — all exist |
| 33 | C2 | — | NO-ENUM | graph enumerated from ref at run time (method) |
| 34 | C3 | Effect {allow, deny, ask}; Reply {once, always, reject} | CENSUSED | pin re-executed: `permission.ts:55` and `:41` line-exact |
| 35 | C4 | — | NO-ENUM | leaf keys data-driven from ref config schema (C4's method) |
| 36 | C5 | failure classes {server loss, plugin crash, partial write, bad data} | EXEMPT | authored injection classes; dialog-workspace-file-changes named cell covered by C16's trees |
| 37 | C6 | lifecycle verbs | EXEMPT | authored lifecycle matrix |
| 38 | C7 | job-state union {streaming, running, completed, error} | CENSUSED | pin re-executed: session-message.ts:127/:133/:140/:148 line-exact |
| 39 | C8 | R2 verbatim op list + v8 census cure | CENSUSED (cured v8) | re-executed: `grep -n 'Effect.fn("Worktree\.'` = create/remove/refresh/list at :204/:257/:273/:330; session_move at opencode.ts:110 |
| 40 | C9 | MCP verbs {add, connect, auth, fail, restart, remove} | EXEMPT | authored lifecycle matrix; threshold note v8 stands; **MCP PROMPTS cells re-owned here per the C14(b) cure** |
| 41 | C10 | auth flows | EXEMPT | authored matrix |
| 42 | C11 | method list (7) | CENSUSED | pin re-executed: `updater.ts:14` = `["curl","npm","pnpm","bun","yarn","vp","brew"]` — 7, exact |
| 43 | C12 | first-run shapes | EXEMPT | authored matrix |
| 44 | C13 | sources {directory, url, embedded} | **VALID→PINNED** (this round) | `schema/src/skill.ts:45` union = exactly the three; normalize.ts:243-244 re-executed; pin added (§2.5) |
| 45 | C14 | (a) AGENTS.md hierarchy; (b) source enum {command, agent, prompt} | **CURED (this round, MG-4)** + CENSUSED (a-note) | full census in §1: command.ts:159; agent.ts:22-23; no prompt plugin; instruction.ts:38/:48/:72; initialize.txt:14 = prompt copy |
| 46 | C15 | triggers {mention, directory-completion, recents}; tabs {shell, subagents, terminals} | EXEMPT | authored matrix (tab files ride C16-family trees) |
| 47 | C16 | dialog trees (23 ∪ 4 ∪ session-ui) + named dialogs | CENSUSED + NAMED-CELL-VERIFIED | re-executed: `ls -d …/component/dialog-*` = 23; `ls -d …/routes/session/dialog-*` = 4; every named cell existence-checked: dialog-stash/-experiments/-debug/-status/-shell-output.tsx, retry-provider.tsx, masked-text.ts, command-palette.tsx, session-tabs-rail.tsx, session-ui/src/pierre/{commented-lines, comment-hover, diff-selection, file-find}.ts — all exist |
| 48 | C17 | PTY verbs x surfaces | EXEMPT | authored lifecycle matrix |
| 49 | C18 | handler census | CENSUSED | re-executed: `ls -1 …/cli/src/commands/handlers/` = 18 non-test top-level entries = exactly the pin's list (acp, api, auth, debug, default, mini, models, pair, plugin, reload, run, serve, service, session, mcp, stats, uninstall, upgrade; api.test.ts is a test file, correctly outside) |
| 50 | C19 | integration providers | RUNTIME-NAMED | `schema/src/integration-id.ts:3` branded string re-verified; integration.list API census stands |
| 51 | C20 | tool dirs (6 + 15) | CENSUSED | re-executed: `ls …/core/src/tool/` = read-filesystem, mcp, html-markdown, http-body, runtime, patch.txt (+ plugin dir + AGENTS.md — doc-file note added, M-5); `ls …/tool/plugin/ \| wc -l` = 15 |
| 52 | C21 | — | NO-ENUM | — |
| 53 | C22 | env-read set (3 vars); OTLP protocol detail set | CENSUSED | re-executed: observability.ts:22-23 + otlp.ts:30 + cli/index.ts:126-128 = exactly the 3 var names; ours `grep -ri 'OTEL_\|otlp' crates/` = 0 |
| 54 | D1 | consumers {TUI, webui} | EXEMPT | authored matrix |
| 55 | D2 | — | NO-ENUM | endpoints/error codes data-driven from ref server (method) |
| 56 | D3 | disposition values {parity, absent-API, plugin-never-calls} | EXEMPT | authored adjudication classes |
| 57 | D4 | — | NO-ENUM | tables/columns data-driven from ref schema |
| 58 | D5 | — | NO-ENUM | invariants enumerated from ref at run time |
| 59 | D6 | desktop {main, preload, renderer, shared}; app subpackages | CENSUSED + NAMED-CELL-VERIFIED | re-executed: `ls …/desktop/src/` = exactly those 4; `ls …/app/src/` contains every named subpackage (composer, home, new-session, desktop, …) |
| 60 | D7 | — | NO-ENUM | ACP methods enumerated per-method from ref adapter (method) |
| 61 | D8 | — | NO-ENUM | formats/destinations data-driven |
| 62 | D9 | variant kinds {effort, toggle, budget_tokens} | DATA-DRIVEN-NAMED | catalog-data: models-dev snapshot per I10; `schema/src/model.ts` untyped overlays re-confirmed |
| 63 | D10 | codemode/browser surfaces | EXEMPT | authored family axes |
| 64 | D11 | method classes (named exports, signatures, aliases) | EXEMPT | authored diff-method classes |
| 65 | E1 | interaction x load cells | EXEMPT | authored matrix |
| 66 | E2 | surface/config/data variants; measured 5250/5300, 2300/2400 | EXEMPT + MEASURED | variants authored; the numbers are committed R7 artifacts (tools/parity/spec-p3/), consistent clause↔round-log — measured facts, not set summaries |
| 67 | E3 | session shapes | EXEMPT | authored matrix |
| 68 | E4 | families {transcript, session-list, search, diff} x magnitudes | EXEMPT | authored matrix |
| 69 | E5 | WCAG 4.5:1 / 3:1 | EXEMPT | standard thresholds (authored), not ref facts |
| 70 | E6 | locale classes | EXEMPT | authored matrix (env classes shared with B7) |
| 71 | E7 | error-path union of {C5, D2, C18} | DERIVED-FROM-CENSUSED | all three sources censused/bounded (rows 36, 55, 49); the union is mechanical; placeholder corpus authored |
| 72 | E8 | — | NO-ENUM | corpus/attack classes authored per method |
| 73 | E9 | residue classes | EXEMPT | authored audit classes |
| 74 | E10 | metric set | EXEMPT | authored metrics |
| 75 | E11 | idle states | EXEMPT | authored matrix |
| 76 | E12 | query classes | EXEMPT | authored corpus classes (corpus committed, I9-labeled) |
| 77 | F1 | — | NO-ENUM | matrix cells derive from other clauses' enumerations |
| 78 | F2 | standing accounting entries + util/observability export names | NAMED-CELL-VERIFIED | re-executed: `grep -n export util/src/observability.ts` — Observability namespace (:1), Options (:11/:18), layer (:20) all exist; devtools entry verified R5; the five R10 entries pinned in I10b |
| 79 | F3 | client pairs {TUI+TUI, TUI+webui} | EXEMPT | authored matrix |
| 80 | F4 | degradations {slow stream, timeout, drop, offline} | EXEMPT | authored matrix |
| 81 | F5 | — | NO-ENUM | profiles delegate to E-clauses |
| 82 | F6 | seed pairs (5) | EXEMPT | authored seeds |
| 83 | F7 | trace scope {events, files, sockets, env reads, network calls} | EXEMPT | authored scope classes |

### 3.2 Sweep totals

**83/83 rows** (mechanically enumerated per §3.0 step 1; row count = clause
count). By class: **CENSUSED 12** (I10b, A5, A6, C3, C7, C8, C11, C16,
C18, C20, C22, D6) · **CURED 1** (C14) · **VALID→PINNED 1** (C13) ·
**RUNTIME-NAMED 1** (C19) · **DATA-DRIVEN-NAMED 2** (A3, D9) ·
**NAMED-CELL-VERIFIED 4 primary** (I5, C1, F2, + B7/D6/C16/F2 carry
secondary named-cell verifications) · **VALUE-PINNED 1** (I3) ·
**EXEMPT-BY-CLASS 39** (incl. mixed rows whose enumerations are all
authored) · **NO-ENUM 22**. Every ref-enumerating item is censused,
pinned, cured, runtime-named, data-driven-named, named-cell-verified, or
value-pinned; every authored item is exempt BY CLASS with the class named;
every absent item is adjudicated absent, in the table. **No clause is
skipped — the curated-sweep escape hatch is closed.**

R9-vs-R10 delta (honest): R9's §2 table adjudicated 30 rows (26+ clauses,
list-bearing only). R10 adjudicates 83. New findings vs R9: **C14 (MG-4,
cured)**, C13 (uncited source enum — real, now pinned), C20 (AGENTS.md
census entry unnamed — noted), A6 (count incoherence — fixed), B7/C1/C16
named cells (verifications now recorded in the sweep artifact rather than
left implicit), I3 (value pins now recorded line-exact in the artifact).

---

## 4 — Numeric-annotation audit (every numeral summarizing an enumerated set, M-4 generalization)

| Numeral | Location | List it summarizes | Census re-executed | Verdict |
|---|---|---|---|---|
| 35; 6 + 29 | I10b pin | 35-package universe | `ls -1 …/packages/ \| wc -l` = 35; 6 exhaustive names verified; 6+29 = 35 | EXACT |
| "exactly {home, prompt, sidebar, system}" | A5 pin | 4 slots | `ls` = 4 | EXACT |
| **"all 12 listed panels"** | A6 pin (v8) | 13-item enum | grep = 17 = 13 panels + 4 plumbing | **WRONG → fixed to 13 (M-4)** |
| six kinds | A8 | mermaid kind union | diagnostics.ts:1 = 6 literals | EXACT |
| 12 / 10 / 18 / 7 / 7 / 7 | A8 | per-kind file counts | per-kind `ls \| wc -l` = 12/10/18/7/7/7 | EXACT |
| core/ 15 files | A8 | shared core file count | `find merman/src/core -type f \| wc -l` = 15 (ls shows 14 entries — `color/` is a dir of 2; count form recorded in §3.1 row 21) | EXACT (form pinned) |
| 4 more sub-families | A8 | markdown/palette/plugin/layout-audit | all 4 exist | EXACT |
| {latex, math}; two registrations; two builtin | A8 | fence languages; renderer universe | grep -c = 2; packages-wide grep = 2 sites | EXACT |
| KaTeX 0.16.47; 100k / 256; 5 symbol tables | A8 | version; limits; symbol tables | ui/package.json:117; limits.ts:3-4; symbols.ts:209/:219/:236/:272/:300 | EXACT |
| Effect 3 / Reply 3 literals | C3 pin | literal sets | :55 / :41 | EXACT |
| 4-state union | C7 pin | status union | :127/:133/:140/:148 | EXACT |
| "exactly four" + session_move | C8 cure | worktree ops | grep = 4 (:204/:257/:273/:330) | EXACT |
| 7 methods | C11 pin | updater methods | updater.ts:14 = 7 items | EXACT |
| 23 dirs ∪ 4 | C16 pin | dialog trees | re-counted 23 + 4 | EXACT |
| 18 top-level handlers | C18 pin (list form) | handler census | ls = 18 non-test entries = pin's list | EXACT |
| TWO dirs; 6 + 15 | C20 pin | tool universe | 6 tool entries + plugin dir; 15 in plugin/ | EXACT |
| 3 env vars | C22 | env-read set | 3 names, line-exact | EXACT |
| 4.5:1 / 3:1 | E5 | WCAG thresholds | standard constants, authored | N/A (authored) |
| 5250/5300; 2300/2400 | E2 + round log | measured baselines | committed R7 artifacts (spec-p3); clause↔log consistent | MEASURED (not a set summary) |
| 41 steps / 2 scripts | I6 | RESCAN §4 history | evidence-cited (I9 provenance), not re-derivable | EVIDENCE-CITED |
| 52+30+20+12+15+11+17+5 | header/legend/appendix | appendix rows | row-counted this round = 162/162 | EXACT |

**Result: one wrong numeral in v8 (A6 — M-4, fixed); every other
numeric annotation reproduces or is class-exempt/evidence-cited.**

---

## 5 — Zero-loss re-check (measured, not trusted)

- **Appendix arithmetic (row-counted this round)**: R1 52 · R2 30 · R3 20
  (12+8) · R5 12 · R7 15 · R8 11 · R9 17 · **R10 5** = **162/162**;
  header claim, legend, and round-log chain (102 → 114 → 129 → 140 → 157
  → 162) all updated and consistent.
- **Clause universe unchanged**: 83 clauses (mechanically enumerated,
  §3.0 step 1 = 83) — R10 added NO new clause, renumbered NOTHING (C14
  cure appended in-clause; all other changes are riders/pins/entries
  inside existing clauses).
- **Bidirectionality**: every R10-# tag maps both ways (grep-verified):
  R10-MG4 → C14 cure + I10b guard · R10-SW1 → I10b guard + C13 pin ·
  R10-M-4 → A6 · R10-R-G1 → I10b five entries · R10-M-5 → C20 note +
  I10b identity note. Appendix R10 section = 5 rows, each carrying the
  same tags.
- **Verbatim carry**: every quoted cure block in §2 is a character-exact
  extract of the v9 spec text. The C14 R2/R3 verbatim [G] texts and the
  v8 guard text are RETAINED in place with their cures appended (the C8
  precedent); no round text was deleted.
- **No stale claim introduced**: the only v8 sentences superseded are
  A6's numeral (corrective note in place) and C14(b)'s implicit enum
  validity (same-clause cure, C8-pattern); grep for the false premises —
  "all 12 listed panels" now appears only inside the corrective note and
  the guard's defect-class example; "{command, agent, prompt}" survives
  only as the R2 verbatim carry + the cure that re-dispositions it.
- **All v8 riders intact**: E6/E9/C7/B3 R8 riders, C22, I2b, I5 R9
  extension, E2 legs amendment, R-E entries — spot-verified by grep
  during integration (no removals; additions only).

---

## 6 — Declaration

**docs/scan-spec.md v9 = FINAL candidate 4.** MG-4 is cured at true
discovery-census granularity (commands/agents globs pinned line-exact;
prompt re-dispositioned to its real runtime family; the (a) leg's source
universe pinned and its prompt-copy decoy pre-empted); the guard is
strengthened to cover every enumeration FORM (unions, kinds, sources,
named-cell inventories, count annotations) with count-coherence and a
mandatory exhaustive-sweep rule whose first execution is this round's
83/83 table with a re-runnable discovery procedure; M-4 fixed and the
numeric-annotation audit is clean otherwise; R-G1's class-asymmetry is
cured (five census-cited standing entries); M-5's two nanos closed.
Appendix 162/162; clause universe 83; zero-loss verified.

Per docs/scan-program.md: the ratification counter RESETS — **RATIFY-1‴
and RATIFY-2‴ follow on FINAL candidate 4** (fresh stances, fresh
samples, plus regression checks on the R10 cures: the C14(b) census
re-disposition, the strengthened guard's exhaustive-sweep rule — the
next ratifier should RE-RUN §3.0's discovery procedure and diff against
§3.1's table — and the five R-G1 entries' census commands).
