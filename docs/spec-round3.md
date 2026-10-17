# SPEC-R3 — Round-3 review of the scan spec (v3) — P1 FEATURE-SPACE FOCUS

**Lane:** SPEC-R3 (worktree `.worktrees/lane-spec3`, branch `lane/spec3`). **Date:** 2026-10-17.
**Input under audit:** `docs/scan-spec.md` **v3**, fresh eyes, P1 track per the R2 convergence table.
**Method:** full bottom-up enumeration of the ref tree — **all 35 packages** under `/opt/opencode-ref/packages/`
(not just the 6 named in dispatch), every export class (components, feature-plugins, routes, services,
hooks, commands, tools, protocols), each accounted against v3's A–F clauses at family granularity;
ours (`crates/oc-*`) cross-checked per family. All file paths below are listed, not remembered.

## Headline

- **Unowned ref families found: 12** (7 shippable-gap, 5 scope/accounting). Every one is file-verified.
  Two of them are **R2 proposals that v3 dropped in integration** (A9 images, D9-Code-Mode — v3 reused
  the D9 number for model/variant). The integration-loss class R2 diagnosed in v2 **recurred in v3**.
- **Enumeration-universe gap (biggest structural finding):** the spec never pins WHICH packages are in
  scope. Dispatch itself names 6; the ref has 35. Every family below that escaped R1+R2 lives outside
  the 6-package universe (cli, codemode, desktop, ai, sdk…). I10 pins the commit, not the scope.
- **Adequacy of v3's 8 new clauses: 0 ADEQUATE as written · 5 NOMINAL · 3 WEAK** under the
  cheapest-legal-evidence stress. Common losses vs R2's proposals: interaction phase dropped from [G],
  enumeration anchors not filesystem-derived, unbounded matrices without folding rule.
- **I8 canary: executable for 4/5 sampled axes as designed below; needs tiering** — as written, "inject
  a known defect into one axis" is ambiguous (one per round vs per axis) and either reading is broken
  (too cheap / doubles cost).
- **P1 verdict: ONE more round** (v4 integration + one verification sweep), then P1 is done — see §5.

---

## 1. Unowned-family register (P1 exhaustion pass)

Scope swept: `packages/{core,tui,cli,session-ui,schema,plugin}` exhaustively (src trees listed in full),
plus the other 29 packages at family granularity (`ai app client codemode console containers desktop
effect-drizzle-sqlite enterprise function gui-extensions httpapi-codegen http-recorder identity latex
merman posts protocol script sdk server simulation stats storybook theme ui util web`).
Owned-set confirmed adequate at family granularity: A1–A7 (render/theme/mini/anim), B1–B7 (input/env),
C1–C12 (state families), D1–D8 (data/transport), plus the 8 v3-new clauses for their named families.
The register below is everything that found **no owning clause** after folding attempts.

| # | Family | Ref evidence (verified) | Ours | Proposed disposition |
|---|---|---|---|---|
| U1 | **Images** (gen/attach/preview/cache/diff-image) | `tui/.../dialog-image-preview.tsx`; `feature-plugins/system/diff-viewer-image.tsx`; `session-ui/components/{image-preview,markdown-image}.tsx`; `ai/src/{image,image-client}.ts`; `core/src/image/photon*`; `cli/src/services/retained-image.ts`; `mini/stream-v2.image.ts` | ships: `oc-tui/src/{image,image_blocks,image_display_tests}.rs`, `oc-ai/test-fixtures/.../fal-images` | **Restore R2's A9** (dropped in v3; grep "image" in v3 = 0 hits). This was R2-S1-9 MISSING → still MISSING. |
| U2 | **Code Mode + plugin-browser** | `packages/codemode/*`, `packages/plugin-browser/*` (proxy/tunnel/connection/rpc/files), `core/src/codemode/` | none | **Restore R2's proposed D9 as D10** — v3 reused the D9 number for model/variant and dropped the family (grep "codemode" = 0). R2-S1-6 recurring. |
| U3 | **Terminal / persistent-PTY** (user-facing shell feature) | `core/src/{pty,persistent-pty}/` (pty daemon, bindings), `tui/routes/session/composer/terminals-tab.tsx`, `tui/.../terminal-pane.tsx`, `dialog-shell-output.tsx`, `client/src/pty-handoff.ts` | ships: `oc-server/handlers/{pty,pty_state,persistent_pty,shell}.rs`, `oc-tui/terminal_proto.rs` | **New C17.** B5/B6 own terminal *events/matrix aimed at our UI*; nobody owns the embedded-terminal feature (spawn/resize/scrollback/exit-status/handoff/daemon). C16 catches only the output dialog. |
| U4 | **CLI command surface** (~45 commands) | `cli/src/commands/handlers/*` incl. `service/{start,stop,restart,status,get,set,unset}`, `debug/{agents,config,paths,redact}`, `api`, `reload`, `uninstall`, `pair`; `cli/src/services/{standalone,service-registration,server-connection}` | `oc-cli` | **New C18.** F1's "command x route x state" reads as TUI palette; no clause owns CLI arg/output/exit-code/non-TTY parity. Includes the **background-service/daemon lifecycle** family (start/stop/restart/status + registration), which is a user-visible feature, not an E-track concern. |
| U5 | **Noninteractive / `run` mode** | `cli/src/run/{run,noninteractive,ui,v1}.ts` | partial (`run` refs in oc-cli) | Fold into C18 as a named cell-family (headless exec: stream format, exit codes, scriptability). |
| U6 | **Integrations family** | `core/src/integration/connection.ts`, `tui/.../dialog-integration.tsx`, `cli/src/ui/integration-picker.ts` | ships: `oc-server/handlers/integration.rs` | **New C19** (connect/disconnect/auth-state per integration provider x surface). Both trees ship it; zero clauses name it. |
| U7 | **Built-in tool family** (per-tool behavior parity) | `core/src/tool/*` (`read-filesystem`, `mcp`, `html-markdown`, `http-body`, `runtime`, plugin tools), `core/src/{websearch,file-mutation,file-access,file-retention}.ts`, `core/src/vcs/patch.ts`, `tool-output.ts`; renderers `session-ui/src/tools/*`, `tui/.../tool_rows`-class | ships: oc-server tools, `oc-tui/tool_rows.rs` | **New C20.** C3 owns permission-per-tool, A8 owns output renderers, F2 owns export existence — per-tool arg-schema/behavior/output parity is owned by nobody. |
| U8 | **Stats/telemetry surface** | `tui/feature-plugins/system/{stats,stats-data}.tsx`, `packages/stats/` (hosted app), `cli/.../handlers/stats.ts` | none found | Absent-justified accounting entry (F2/I7) + one C-cell if/when shipped. |
| U9 | **Account / identity / enterprise** | `packages/{identity,enterprise}/`, `core/src/account/`, `core/src/managed-policy.ts` | none | Out-of-parity-scope candidate — but the spec must SAY so (see U12). `managed-policy.ts` interacts with C4 config precedence and must not be silently ignored there. |
| U10 | **Desktop + app frontends** | `packages/desktop/{main,preload,renderer,shared}`, `packages/app/*` (composer/home/new-session/desktop.ts) | none | D6 amendment: name desktop shell explicitly (native chrome, protocol handlers, desktop-side update) or mark absent-justified. |
| U11 | **Determinism / idempotency** | (behavioral family; R2-S2-13) | — | **R2 proposed C17; v3 dropped it** (grep "determin" = 0). Still the S2-13 MISSING gap. |
| U12 | **Enumeration-universe pin** | 35 packages vs 6 in dispatch | — | **I10b / F2 amendment:** pin the package scope per round: {in-scope list, out-of-scope-with-one-line-justification list}. U8–U10 escaped R1+R2 *because of* this hole. |

**Fold-amendments needed (owned, but the owning clause's letter misses the sub-family — will be
inventory-grade without a named cell):** session `fork` (`routes/session/dialog-fork.tsx`; ours:
fork in oc-server) + `anchors`/`anchor-view` + `grouping/{session,tree}` + `message-navigation` → C1/C2
cells; composer tabs `shell/subagents/terminals` → C15 (C15's [G] covers triggers only); 
`thinking-syntax.ts` → A8 family list; `ssh-askpass.ts` → B7 cell; feature-plugin SLOT architecture
(`feature-plugins/{home,prompt,sidebar,system}`) → C6/A5 amendment (internal slots are surfaces);
`file-retention/file-mutation` + `dialog-workspace-file-changes` review flow → C16/C5 cell;
`editor-zed*`, `attention-sounds.{bun,node}` backend split → B7/E-track cells already implied.

**Count: 12 unowned + 8 fold-amendments. Zero families left unaccounted after these dispositions**
(the register accounts every src dir listed across all 35 packages).

---

## 2. Adequacy of v3's new clauses (A8/B8/C13–C16/D9/F6/F7)

Stress = R2's S5: *what is the cheapest evidence that satisfies the clause's letter?*

| Clause | Verdict | Cheapest legal pass | What the letter loses |
|---|---|---|---|
| A8 rich content | **WEAK** | Inventory ref's merman/latex packages + render one trivial diagram per size bucket; "capability inventory" satisfied by grep. | No theme dimension (syntax-theme x renderer interaction); no corpus provenance (I9 unbound); no absent-justified accounting for OUR missing families (mermaid/latex — R2's own text had it); sub-families unnamed (merman: flowchart/gantt/gitgraph/sequence; latex: inline vs block). |
| B8 width | **NOMINAL** | Diff unicode-width crate version numbers ours vs ref ("version diff" satisfied by a changelog quote). | No pass criterion (the assertion must be cursor-cell position vs ref on the same string, per codepoint class); no corpus definition; no surface binding (wrap.rs? input echo? diff viewer? mini?). |
| C13 skills | **WEAK** | Enumerate skill sources, open dialog-skill, capture. | R2's source x phase matrix collapsed: {directory,url,embedded} x {discovery,permission,invocation} → "per skill surface". Invocation-through-agent and URL-fetch failure paths escape. |
| C14 commands/agents/prompts | **NOMINAL** | Place one `.opencode/command/foo.md`, assert it appears before global; pass. | "per source" under-enumerated (command/agent/prompt x project/global/user/embedded); no frontmatter-field-validation cell (unknown field silently ignored = the R2 catch); agent-mode binding + catalog/frecency UI dropped from [G]. |
| C15 composer context | **NOMINAL** | Trigger @-mention popup, assert popup contents ("completion assert"). | Payload resolution unowned — R2's exact catch ("mention resolves to wrong path payload with a correct-looking chip") is passable. history/stash/frecency named in prose, absent from [G]. Composer tabs missing (fold above). |
| C16 long-tail dialogs | **WEAK** | Open/close every enumerated dialog once, capture each. | Enumeration anchor: "every DialogKind" — no single `DialogKind` symbol exists (grep); surfaces live in THREE trees (`tui/src/component/dialog-*`, `tui/src/routes/session/dialog-*`, `session-ui`). And "per dialog x state" has no action-assert — stash must actually stash, rename actually rename. |
| D9 model/variant | **NOMINAL** | Enumerate catalog for 2 providers, assert config round-trips. | Matrix unbounded (providers x models = hundreds) with no folding rule; "models-dev drift" has no method (diff against WHAT pin — I10 must bind); model-preference persistence + dialog-model/variant UI cells are in prose, not [G]. |
| F6 interaction review | **WEAK** | List axis-pairs of the round's existing findings, write a prose "interaction walk". | Circular: "on the round's findings" — axis-pairs that produced no single-axis finding never get pair-tested, which is precisely the class F6 exists for. No pair-selection rule, no concrete interleaved-execution probe, no artifact. |
| F7 negative-space | **NOMINAL** | One surface, diff event NAMES ours-minus-ref. | Trace instrumentation undefined (files written? sockets? env reads? scope!); diff granularity unspecified (name+count+args); no disposition rule (each extra = finding or justified). |

**Tally: 0 ADEQUATE / 5 NOMINAL / 3 WEAK.** v3's new clauses are strictly better than v2's silence, but
the compression loss R2 measured in v2→v3 integration (30%) repeated v3-internally: R2's §c rewrites
carried source x phase matrices and absent-justified rules that v3's one-line forms dropped again.
**This is the no-summary rule (I11) violating itself in the spec's own integration** — see §5 verdict.

---

## 3. I8 canary executability — 5 concrete designs

I8 as written is ambiguous ("inject a known defect into **one axis**" — per round? per axis?). Per-round
is too cheap (7 axes never canaried in a 7-round program); per-axis doubles round cost. Proposed tiering
in §5. The designs below assume the natural reading (canary before an axis's evidence is accepted) and
confirm feasibility:

1. **A1 (token x state matrix).** Inject: swap two theme-token cells in the capture harness's expected
   map (e.g. `border.focused` ↔ `border.default` on the session frame in `selected` state). Catch: the
   cell-diff must flag exactly that cell; a harness that passes with swapped expectations (or diffs
   nothing because both cells render alike) fails the canary → redesign to per-cell identity, not
   frame-hash-only.
2. **B8 (width).** Inject: perturb the width table for one codepoint class in OUR build (e.g. treat
   U+2764 VS16 sequence as width-1) and one East-Asian Ambiguous char as width-2. Catch: the
   cursor-cell assertion vs ref on the corpus must desync on exactly those classes — proving the method
   measures rendered position, not table metadata. If the sweep still passes, it was a version-string
   diff in disguise.
3. **C4 (config round-trip).** Inject: make one leaf key's write path silently drop a value (write
   default instead of user value) for a nested key under a JSONC comment. Catch: the per-leaf
   write→restart→read assert must fail on that key AND the JSONC-preservation check must flag the
   comment if the writer round-trips through plain JSON. Kills both fixture-happy (never restarts) and
   adjudication-masking (rereads in-process cache) grades.
4. **D1 (SSE).** Inject: reorder two event types in a recorded real stream (tool result before its
   tool call) and drop one optional field from a low-frequency type. Catch: the publish→consume trace
   must fail on ordering for the affected consumer and on field-presence for the schema check —
   proving the method consumes REAL streams field-wise, not replays a curated fixture.
5. **F7 (negative-space).** Inject: add one extra fs write (touch a state file on every keypress) and
   one extra network call (telemetry ping on boot) to OUR build. Catch: the ours-minus-ref trace diff
   must list both extras. If the diff is name-only over event types, the injected write (same name,
   higher count) must still trip it — count+args granularity is the canary's real assertion.

Feasibility: all 5 are cheap (<10 min each), injectable at source or harness level, and each names its
redesign trigger. I8 is executable per axis **once tiered**; the canary corpus should itself be
committed as a standing artifact (bind under I5 with `golden_tests.rs`-class instruments).

---

## 4. P1 convergence verdict

**ONE more round, not done.** Basis:

- The family enumeration is now plausibly exhaustive (filesystem-driven, all 35 packages, zero
  residual unaccounted dirs after §1 dispositions). The *discovery* half of P1 is done.
- But v4 must integrate 12 register items + 8 fold-amendments + the §2 rewrites, and this round
  demonstrated that our integration step **loses granularity by default** (2 of 8 v3-new clauses were
  R2 proposals whose numbers got reused; 3 of R2's proposed clauses — A9, C17-determinism, D10-SDK —
  silently vanished). Until one integration round lands verbatim-quoted and is re-stressed, P1 cannot
  be declared converged.
- SPEC-R4 (P1) should therefore be: integrate §5 diff with verbatim clause text + gap-ID provenance
  (R3-U1..U12), then a short stress-sweep of ONLY the new/reworded clauses. If that sweep finds no new
  unowned families and ≤2 nominals, P1 closes at R4.

## 5. Proposed v4 diff (P1 slice; P2–P5 tracks keep their own rounds)

New clauses (each [G][F][M]; verbatim-ready):

- **A9 Images (restore R2):** [G] {paste, attach, tool-output, gen, preview-zoom, retained cache,
  diff-viewer-image, mini stream-v2.image} x format {png,jpg,webp,gif} x size class.
  [F] I1+I4 [M] capture diff per cell; ours-ships cells vs ref parity, absent-justified per missing.
- **C17 Terminal family (U3):** [G] {spawn, resize, scroll, exit-status, handoff, daemon-restart} x
  {terminals-tab, terminal-pane, shell-output dialog, pty handlers} x TTY state.
  [F] I1 real ptys [M] session drive + capture + exit-status assert.
- **C18 CLI/service/run (U4+U5):** [G] every `cli/src/commands/handlers/*` command x args-shape x
  output {tty, non-tty} x exit code; service lifecycle {start,stop,restart,status,get,set,unset,
  registration, standalone}; `run` {interactive, noninteractive, v1} x stream format.
  [F] I1+I10 [M] executed-command matrix artifact (every cell dispositioned).
- **C19 Integrations (U6):** [G] per integration provider x {connect, disconnect, auth-state} x
  {dialog, picker, handlers}. [F] I1 [M] flow assert + credential store check.
- **C20 Tool family (U7):** [G] per built-in tool x arg-shape x output-kind x renderer x permission
  class x side-effect. [F] I1+I2 [M] schema+behavior+render parity vs ref per tool.
- **C21 Determinism (restore R2's C17/U11):** rapid/duplicate input x double-submit guards x
  idempotent re-run x event-order stability; I3 vt-clock driven.
- **D10 Code Mode + plugin-browser (restore R2's D9/U2):** interpreter/stdlib/openapi/tool-runtime
  surface x parity; browser tunnel/proxy/connection round-trips. [F] I2 [M] per-API trace.
- **D6 amendment (U10):** add desktop shell + app package to the served-surface enumeration or mark
  absent-justified.
- **I10b scope pin (U12):** round report lists in-scope packages + out-of-scope with one-line
  justification; F2's "every export" is quantified against that list.
- **I8 tiering:** per-round one random-axis canary + per-axis canary on first evidence + re-canary on
  instrument change; canary corpus committed under I5.

Rewrites of the weak/nominal new clauses (full [G][F][M] text in §2's "loses" column, condensed):
A8 += theme dimension, I9 corpus provenance, absent-justified per missing renderer, merman/latex
sub-families; B8 += cursor-cell-vs-ref pass criterion + named corpus + surface binding; C13 += restore
{directory,url,embedded} x {discovery,permission,invocation}; C14 += explicit source enum +
frontmatter-field validation + agent-mode binding cells; C15 += payload-resolution + insert-format
cells + tabs fold; C16 += filesystem-union enumeration (3 trees) + per-dialog action-assert;
D9 += provider-CLASS folding rule + models-dev pin via I10 + preference/UI cells in [G]; F6 +=
risk-ranked standing pair corpus + designated pairs incl. no-prior-finding pairs + probe artifact;
F7 += trace scope definition + name/count/args granularity + disposition rule.

Non-P1 items registered for their owning tracks (not expanded here): B9 remote/container restore and
E10/E11 (P2/P3), SDK/client D11 (P1-adjacent, fold decision to R4), instrument binding under I5 (P4).

## Round log (this lane)
- v3 audited P1: 35-package sweep, 12 unowned + 8 folds, 8 new clauses stressed (0/5/3), 5 canaries
  designed, P1 verdict = one more round. v4 diff proposed above.
