# The Scan Specification (扫描细则) — LIVING DOCUMENT

Version: **v8** (SPEC-R9 integration round — RATIFY-1′'s material gap MG-3
cured: A8 rewritten at census-pinned family granularity, list-census guard
added to I10b, full list-sweep of every list-bearing clause; FINAL
candidate 3).
Protocol:
docs/scan-program.md. Lineage: v1 (R5 framework) -> SPEC-R1 (52 gaps;
taxonomy-v2 [G][F][M]) -> v2 -> SPEC-R2 (30 gaps; [G][F][M] keystone;
I8-I11) -> v3 -> SPEC-R3 (12 unowned families U1-U12 + 8 fold-amendments +
clause rewrites) -> v4 -> v5 (SPEC-R5's 10 E-clause rewrites +
3 amendments; E-axis executable; baselines ratified in
docs/spec-round6.md) -> v6 (SPEC-R7: P3+P4+P5 convergence; FINAL
candidate 1, FAILED ratification in docs/ratify-1.md) -> v7 (SPEC-R8: cures
applied per docs/ratify-1.md — MG-1 telemetry/observability
family owned by new clause C22 + F7 pre-disposition rewritten + B7 OTEL_*
cells + F2/I8 amendments; MG-2 I10b enumeration pin corrected to the true
35-package list with a mechanical census command; residual riders
E6/E9/C7/B3; declared FINAL candidate 2 in docs/spec-round8.md) ->
**THIS v8** (SPEC-R9: RATIFY-1′ FAIL per docs/ratify-1b.md — MG-3 A8
mermaid/latex enumeration cured by a complete merman+latex census and an
A8 rewrite at true family granularity; I10b list-census guard extends the
MG-2 pin discipline from packages to every ref-enumerating in-clause list;
full v8 list-sweep pinned or exempted every list-bearing clause; R-E/R-F/
M-1..M3 residual riders; declared FINAL candidate 3 in docs/spec-round9.md).

**Integration rule (binding, per I11 — this round's correction):** clause text is
carried VERBATIM from the round documents — R1 taxonomy-v2 rows, R2 §c rewrites,
R3 §5 proposals, R5 §3 proposals (v5), R7 §2/§3/§4 proposals (v6), R8 cures
(docs/ratify-1.md proposed cures: the F7 re-disposition and the four residual
riders quoted character-exact; C22 and the census pin are R8-authored and
carried verbatim in docs/spec-round8.md), R9 cures (the A8 rewrite, the I10b
list-census guard, and every census pin / rider below are R9-authored and
carried verbatim in docs/spec-round9.md). Where v3 compressed or dropped a round's
text, the fuller round text is RESTORED, not summarized. Every clause carries **[G]ranularity /
[F]idelity / [M]ethod** — an agent satisfying the letter must also satisfy the
spirit — plus its source gap IDs `{…}` (R2-M-4). The appendix maps ALL 157 gap
IDs (R1 52 · R2 30 · R3 12+8 · R5 12 · R7 15 · R8 11 · R9 17) to clauses/folds/justifications.

## Standing instrument layer (binds EVERY clause)
I1 REAL payloads end-to-end — no hand-set ids; chains from our own
   server/plugin-host outward (self-consistency). {R1: S4-2, S4-5}
I2 REAL plugin sandbox; **≥3 ecosystems + omo-slim first (restored)**, incl.
   marketplace fetch; ref+plugin = ground truth for plugin surfaces.
   {R1: S3-4, S4-3; R2: (a)S3-4, (a)S1-11}
I2b **Plugin-era ground-truth binary**: for every plugin-surface comparison
   (I2's ref+plugin pairing, A5, C6, D3, and any clause touching plugin-
   rendered or plugin-registered content), the ground-truth ref = the **ref
   2.0.25 binary** (the user's `/root/.opencode/bin/opencode`) + the current
   plugin (omo-slim first), NOT the pinned 2.0.24 source tree: 2.0.24
   **cannot load the current plugin generation** (`Cannot find module
   'oh-my-opencode-slim/rpc'`; plugin shows failed in /plugins; evidence:
   docs/rescan-findings.md §3-F10, ref-home log err_7b5efd85…d5244f41) —
   any plugin-era parity claim differenced against 2.0.24 is **vacuously
   true** (both sides broken ⇒ no divergence visible). Standing rule: every
   plugin-era comparison records which ref form it used (source-2.0.24 for
   non-plugin source parity; binary-2.0.25 for plugin-era), and an
   instrument gate FAILS when the ref's plugin state ≠ active while ours is
   active (and vice versa). {R7: G1, G2; evidence: RESCAN F10 + §1b/§1c of
   docs/spec-round7.md}
I3 Virtual clock for **ALL timed behavior** (cadence = drive source), not
   animation only: toast pause-on-activity, spinner 80ms (`spinner.tsx`,
   R5 pin), reconnect backoff (ref 1s/5s), leader/chord timeouts,
   StartupLoading 500ms/3s gates, debounces, update-check interval,
   attention bell timing. {R1: S4-4; R2: (a)S4-4; R5: §3-I3}
I4 Token-exact cells per element x state.
I5 Falsifiability: every clause names its evidence artifact. Standing in-repo
   instruments are BOUND as evidence artifacts (rounds must not rebuild ad-hoc):
   `golden_tests.rs`, `frame_budget_tests.rs`, `replay.rs`, `mouse_modes.rs`,
   `diff_viewer_golden_tests.rs`, ref `packages/simulation` tapes.
   {R1: S4-7; R2: S4-6, S4-7} **R7 extension (v6):** the list additionally
   binds (R6/R7): `crates/oc-tui/tests/spec_e1_canary.rs`,
   `spec_e5_canary.rs`, `spec_e2_tti.rs`, `spec_e12_first_pass.rs` (+ corpus
   `tools/parity/spec-canaries/e12_search_corpus.json`),
   `tools/parity/spec-canaries/e3_leak_canary.rs`,
   `tools/parity/src/bin/rescan-tui.rs` (the plugin-armed live-capture
   driver — the ONLY standing instrument proven to load the current plugin
   generation on both stacks), and `tools/parity/spec-p3/` (ref/refbin/ours
   TTI PTY probes + results). Rounds must not rebuild ad-hoc variants of
   these. {R7: G8} **R9 extension (v8 — R-F):** the list additionally
   binds C22's mock OTLP collector (loopback HTTP recorder asserting
   /v1/logs + /v1/traces POSTs and zero-traffic-when-unset) —
   clause-declared standing since v7, formally I5-named from v8; the
   first C22 execution commits it in-tree. {R9: R-F}
I6 Miss-attribution ledger per finding (inventory-grade / fixture-happy /
   adjudication-masking). {R1: S4-6} **R7 standing rule (v6) —
   umbrella-adjudication masking (applies to ANY comparison instrument,
   current or future):** an umbrella (b)-class (coarse mismatch classes —
   e.g. coin-flip placeholder, digit drift, indexed color artifact) may
   NOT absorb a step that contains animated/busy/indicator cells; steps
   containing such cells require per-cell family classification. A closure
   may not rest on identical mismatch counts versus a baseline ("19 ==
   19") — it requires per-cell family evidence. Settle-then-capture
   discipline may never be cited for animated-element parity (what the
   capture never observed cannot be adjudicated equal). Evidence base:
   RESCAN §4 (41 corpus steps absorbed across 2 scripts; the "no new
   spinner class" conclusion was false at the element-family level —
   RESCAN F3 proved a real busy-indicator divergence the umbrella had
   swallowed). {R7: G7}
I7 Bottom-up ref enumeration as feature completeness backstop, **quantified
   against the I10b package universe**. {R3: U12}
I8 **Falsifiability canary — TIERED** (v4, per R3 §3/§5): per round one
   random-axis canary + per-axis canary BEFORE that axis's first evidence is
   accepted + re-canary on instrument change; canary corpus committed as a
   standing artifact under I5. Concrete executable designs (R3 §3): A1
   token-cell swap, B8 width-table perturbation, C4 write-path value drop,
   D1 event reorder + optional-field drop, F7 extra-fs-write + extra-network-call
   injection. An axis whose method can't catch the canary is redesign on the
   spot. {R1: S4-7; R2: S4-2; R3: §3} **R7 canary-coverage matrix (v6),
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
   {R7: G9} **R8 design addendum (v7 — MG-1):** DESIGNED additionally =
   **C22** (OTLP endpoint set/unset traffic-gating pair — endpoint set ⇒
   mock collector receives /v1/logs + /v1/traces; endpoint unset ⇒ zero
   collector traffic while the file log-layer still writes; binding before
   C22's first evidence, per the defer-or-design rule). {R8: MG-1}
I9 **Provenance ledger**: every fixture/capture records its source (real-run
   capture vs synthetic); synthetic data is never sole evidence.
   {R1: S4-2; R2: S4-3}
I10 **Ground-truth pin**: ref commit pinned per round; re-baseline on ref
    release; ref+plugin pin recorded with the report. {R2: S4-4}
I10b **Enumeration-universe pin** (R3 U12): every round's report lists (a)
    in-scope packages and (b) out-of-scope packages with a one-line
    justification each; F2's "every export" is quantified against that list.
    Pinned universe (SPEC-R3 sweep; 35 packages under
    `/opt/opencode-ref/packages/`, verified 2026-10-17): exhaustive per-src-tree
    {core, tui, cli, session-ui, schema, plugin}; family-granular {ai app client
    codemode console containers desktop effect-drizzle-sqlite enterprise
    function gui-extensions httpapi-codegen http-recorder identity latex merman
    plugin-browser posts protocol script sdk server simulation stats storybook
    theme ui util web}. **R8 correction (v7, MG-2):** the v6 pin claimed 35
    but enumerated 34 — **`plugin-browser` was missing** (found by
    RATIFY-1's sorted diff; restored above between merman and posts). It is
    behaviorally D10-owned (browser tunnel/proxy/connection/rpc/files), so
    no family was ever unowned, but the F2/I7 completeness backstop
    executes "every export in the I10b-pinned package universe" — an agent
    materializing the universe from the pin LIST never accounted
    plugin-browser's exports. **Census discipline (v7): the pin is
    mechanical, not prose** — `ls -1 /opt/opencode-ref/packages/ | sort`
    must yield exactly the 35 names above (6 exhaustive + 29
    family-granular), and `ls -1 /opt/opencode-ref/packages/ | wc -l` =
    35 (verified 2026-10-17, RATIFY-1 and this round); round reports' (a)/(b)
    lists reconcile against that command's output, never against recall.
    {R8: MG-2} **List-census guard (v8 — R9-MG3; extends this pin's
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
    exempt-by-class. {R9: MG-3} Standing out-of-scope-with-justification entries: **stats** (U8 — we
    ship no stats surface; absent-justified accounting entry under F2/I7 + one
    C-cell if/when shipped); **identity/enterprise** (U9 — not shipped by us;
    `core/src/managed-policy.ts` still binds C4 precedence). Standing
    accounting entries, no parity surface (R4 re-sweep): **console** (scaffold,
    src empty — F2 export accounting only), **posts** (static marketing/docs
    site — no runtime parity surface), **script** (single-file dev script — F2
    accounting), `tui/src/devtools` (dev-only route — owned generically by
    F1's route matrix). Standing accounting entries, no product-side client
    (v8, R-E): **function** (Cloudflare-worker share backend: `/share_create`
    `/share_sync` `/share_poll` `/share_delete[_admin]`, SyncServer
    DurableObject, R2, GitHub-App auth — hosted server-side; no client in
    the pinned 2.0.24 source NOR the 2.0.25 binary, verified 2026-10-17 in
    docs/ratify-1b.md: the `stats`-class; accounting entry only — the share
    product family is D8-owned, a future product client re-baselines per
    I10); **web** (share viewer polling `ws /share_poll?id=` + docs site —
    viewer-side of the same hosted pair; rides D8's destination cells;
    accounting entry only). {R3: U8, U9, U12; R4: re-sweep; R9: R-E}
I11 **Process + output contract**: ≤45-min chunks, no backgrounded scans,
    worktree isolation, per-round output format (gap register + adequacy table +
    convergence estimate), no summary-only dispatch — integrations quote clause
    text verbatim and carry gap-ID provenance; this v4 file is the corrected
    instance after three lossy integrations. {R2: S4-5}

## A — Visual / Rendering
A1 Theme tokens x element x STATE matrix — state set = union {base, hover,
   focus, busy, error, disabled, selected, expanded, interrupted, anim-phase};
   numeric edge sweep. [G] per token x element-cell x state [F] I4 token-exact
   cell capture; ref+plugin pair for plugin-painted cells [M] cell-diff harness
   vs ref frames incl. state injectors; ref token->element enumeration + vt
   capture diff. {R1: S5-1; R2: (a)S5-1}
A2 Geometry (padding/gap/centering/truncation/wrap). [G] per surface
   x size {40,60,80,120,160} + narrow/wide sweep [F] vt grid golden-frame
   capture [M] golden-frame diff at width sweep, rule-by-rule vs ref;
   **threshold = zero unexpected cell diffs per surface x width cell —
   expected-diff classes are enumerated and dispositioned per cell
   (glyph-width ambiguity is B8-owned; theme-token deltas are A1-owned;
   anything else is a finding)**; artifact = per-surface x width diff
   table + the golden corpus committed in-repo. {R7: G10}
A3 Edge + huge data — **R2 §c rewrite, verbatim**: [G] widths {0,1,2,10,60,80,
   120,240} x msgs {0,1,2,10,1k,10k} x full part-kind enum (materialized from
   `oc-schema`, not prose) x wide-char set {pure CJK, mixed, combining, ZWJ
   family, VS16, ambiguous} x overflow mode {truncate, wrap, scroll} per
   surface. [F] I1+I4. [M] golden-frame corpus persisted in-repo (bind
   `golden_tests.rs`), cell table checked into the round artifact.
   {R1: S2-4, S5-2; R2: S5-1, (a)S5-2, (a)S3-5}
A4 Temporal at vt-clock: [G] per-frame content @ every animated element
   (spinners, pulse, shimmer, marquee, fade, scrim alpha) x phase [F] I3 clock +
   frame stepping + drive-source audit [M] clock-stepped frame series capture.
   **V8 clock note (R9-M-3):** the vt-clock drives OUR leg; the ref leg is
   captured at pinned REAL cadences (ref's process is not vt-clocked) —
   stepped-frame comparison happens at the same named cadence sources.
   {R1: S4-4; R9: M-3}
A5 Plugin-rendered surfaces — incl. **internal feature-plugin SLOT
   architecture** (`feature-plugins/{home,prompt,sidebar,system}` — internal
   slots are surfaces; R3 fold 5). [G] every plugin API render slot x state
   [F] I2 ref+plugin pair (≥3 ecosystems + omo-slim) [M] side-by-side capture
   diff. **V8 census pin:** slot list verified against `ls -1
   /opt/opencode-ref/packages/tui/src/feature-plugins/` = exactly {home,
   prompt, sidebar, system} (2026-10-17). {R1: S1-11, S3-4; R2: (a)S1-11;
   R3: FOLD-5; R9: L2}
A6 MINI frontend — **R2 §c rewrite, verbatim**: [G] panel enum from ref
   `mini/` (footer.{command,form,menu,permission,prompt,subagent,view},
   scrollback.{writer,surface}, splash, stream-v2.{fragment,image,subagent}) x
   state x width; fragment corpus = real LLM chunk boundaries. [F] I1+I3.
   [M] mini PTY capture diff vs mini spec (mini/theme.ts quantizer rules).
   **V8 census pin:** panel files verified against `ls -1
   /opt/opencode-ref/packages/tui/src/mini/ | grep -E
   '^(footer|scrollback|stream-v2|splash)'` — all 12 listed panels exist as
   files; `stream-v2.transport.ts` is plumbing, not a panel (2026-10-17).
   {R1: S1-5; R2: (a)S1-5; R9: L1}
A7 Theme-system chain: [G] discovery -> select -> apply -> live-preview ->
   live-retheme -> error chain (ThemeErrorToast) x step; user-theme-dir
   discovery; system detect (OSC 11); oklab quantization; syntax themes.
   [F] I4 + real user theme dirs [M] apply-path trace + full-chain round-trip
   + capture. {R1: S1-13; R2: (a)S1-13}
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
A9 **Images** (RESTORED v4 — R2's A9/R3's U1; v3 had zero "image" hits):
   [G] {paste, attach, tool-output, gen, preview-zoom, retained cache,
   diff-viewer-image, mini stream-v2.image} x format {png,jpg,webp,gif} x size
   class. [F] I1+I4 [M] capture diff per cell; ours-ships cells vs ref parity,
   absent-justified per missing. {R1: S1-9; R2: (a)S1-9, §c-A9; R3: U1}

## B — Interaction / Input / Environment
B1 Keyboard: [G] binding x route x mode x layer x config-variant (incl. user
   keybind overrides; chord/leader timeout values as observable outcomes) x
   state [F] I1 real chain + user keymap files [M] dispatched-action trace
   (end-behavior table), not table presence. {R1: S1-12, S5-3}
B2 Pointer: [G] target x state x shape x **lifetime-phase** (hit-rect validity
   across route/mode change, dialog open/close, busy transitions); drag/hold/
   middle/right; wheel in every scrollable; **selection copy — clipboard
   round-trip of RENDERED content (wide chars, emoji, box-drawing, masked
   text)** [F] I1 real metadata ids + plugin rows [M] hit-inject + observed
   effect + rect-lifetime trace; would catch copy that concatenates cells
   without wide-char reconstruction. {R1: S5-4; R2: S2-5}
B3 Focus/layering/modality stacks. [G] per legal stack [F] routed-input trace
   [M] stack enumeration + key/pointer routing per stack.
   **R8-N4 rider (v7 — ratify-1 R-D):** artifact = per-stack routed-input
   trace table, assert = routing matches ref per legal stack (ours-vs-ref
   columns; key + pointer events land on the same stack-top target as ref,
   per stack). {R8: R-D}
B4 Input methods: [G] per paste form (bracketed/image/large/WHATWG), IME
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
B5 Terminal events (resize/focus/suspend/$EDITOR round-trip/mouse-mode
   transitions) per surface. [G] per surface x event x mid-state [F] capture
   [M] PTY event scripts; inject + state assertion.
B6 Terminal matrix — **R2 §c rewrite, verbatim**: [G] rows {xterm, kitty,
   ghostty/wezterm-class, tmux x {1,2 nested}, screen, no-TERM, 16-color, ssh}
   x cols {kitty-keys, mouse modes, OSC 10/11/12/52/777, DECSCUSR, title,
   selection, color-count fallback, undercurl, synchronized-output}; TM-1..10
   cells absorbed with signed/unsigned status. [F] real emulators (tmux live)
   + I10 pin [M] protocol-signature probe script + behavior capture per cell.
   {R1: S3-1; R2: S5-1, (a)S3-1} **R7 executability rider (v6):** per B6
   cell the runner emits {tool, threshold, environment, disposition}. Tool
   = standing protocol-signature probe script (extend the TM-1..10
   harness) + behavior capture pair (ours vs ref in the SAME emulator).
   Threshold = (i) signature cells: exact response-byte equality for the
   query set; (ii) behavior cells: frame parity ours-vs-ref at equal
   window size in that emulator; a class-level diff (not byte-diff) for
   animation cells per the I6 masking rule. Environment dispositions
   (measured 2026-10-17): LIVE = tmux x{1,2}, no-TERM, 16-color, ssh
   (loopback sshd); DOCKER = screen, xterm-under-Xvfb (container provides
   Xvfb); DEFERRED-WITH-RECIPE = kitty, ghostty/wezterm-class (container
   image per emulator + vt-dump compare; headless-render capability named
   per emulator before the cell may close). Artifact = the cell table
   with disposition column (executed / docker / deferred-recipe),
   committed per round. {R7: G3}
B7 Platform/env matrix — **R2 §c rewrite, verbatim + R3 folds**: [G] OS
   {linux, macOS, windows/ConPTY (mouse/resize/VT semantics, powershell
   profiles, CRLF in tool output, path separators, sound backend absence —
   each a distinct cell)} x env cells {unset TERM, no HOME, no PATH, no git/rg,
   read-only HOME, LANG x {C, en_US.UTF-8, non-UTF-8 (ru_RU.KOI8-R class)},
   TZ absent, OPENCODE_* incl. CLI_CONFIG_CONTENT, OTEL_* (R8-MG1(b) cells
   below), SSH_TTY, CI vars, docker} x
   per-OS {$EDITOR default, sound/notify backend, updater path} + named cells:
   `ssh-askpass.ts` (fold 4), `editor-zed*` (fold 7), `attention-sounds.{bun,
   node}` backend split (fold 8). [F] clean envs; real envs where available
   [M] boot+smoke matrix script, cell table artifact; behavior diff per class.
   {R1: S3-2, S3-3; R2: S3-4, S3-5, (a)S3-2, (a)S3-3; R3: FOLD-4, FOLD-7, FOLD-8;
   R9: M-2}
   **R7 executability rider (v6):** env-cell runner = boot+smoke under
   `env(1)` per cell; assert = boot reaches first frame AND one smoke
   action completes; diff-vs-baseline-class table per cell. Environment
   dispositions (measured 2026-10-17): LIVE = unset TERM, no HOME, no
   PATH, no git/rg, read-only HOME, LANG x {C, en_US.UTF-8, non-UTF-8},
   TZ absent, OPENCODE_* incl. CLI_CONFIG_CONTENT, OTEL_* (the R8 cells
   below; v8 consistency addition, R9 M-2), SSH_TTY, CI vars,
   docker; DEFERRED-WITH-RECIPE = windows/ConPTY (needs a real Windows
   host — wine does not reproduce ConPTY semantics; recipe = CI windows
   runner + the same runner script), macOS (recipe = CI macOS runner);
   sound-backend-absence and per-OS named cells ride their OS
   disposition. Artifact = matrix JSON + per-cell PASS/FAIL. {R7: G4}
   **R8 OTEL_* cells (v7 — MG-1(b)):** the env-cell set additionally
   includes {OTEL_EXPORTER_OTLP_ENDPOINT set, OTEL_EXPORTER_OTLP_HEADERS
   set (valid + malformed k=v list), OTEL_RESOURCE_ATTRIBUTES set (valid +
   malformed W3C list)} — LIVE (plain `env(1)` cells). **Boundary
   (E6↔B7-style): B7 owns boot/survival/smoke under OTEL_* envs; C22 owns
   the observability behavior in them** (env-read trace, emission gating,
   log-layer). Ours today: no OTEL_* reads exist to survive — the B7 cell
   still runs (boot must not regress when the vars are present) and the
   C22 divergence is dispositioned on C22's leg. {R8: MG-1}
B8 **Width implementation** (v4 rewrite per R3 §2): [G] per codepoint class
   {pure CJK, mixed, combining, ZWJ family, VS16, East-Asian Ambiguous} at OUR
   unicode-width version vs ref's width calculation; grapheme clustering;
   user-font cell-metric dependence. **Pass criterion: cursor-cell position vs
   ref on the same string, per class** (a version-string diff is a fail);
   corpus checked in; surface binding NAMED (wrap engine, input echo, diff
   viewer, mini). [F] I10 version-pinned tables [M] corpus sweep +
   cursor-desync diff vs ref; I8 canary = width-table perturbation must desync
   exactly the perturbed classes. {R2: S3-3; R3: §2-B8} **R7 executability
   rider (v6):** tool = committed corpus runner that renders each corpus
   string through both width implementations and compares **cursor-cell
   position** (the clause's pass criterion); corpus = the codepoint-class
   set of the clause, checked in (extends A3's wide-char set). Threshold =
   zero desync on the unperturbed corpus, per class; the I8 canary
   (width-table perturbation desyncs EXACTLY the perturbed classes) runs
   BEFORE the first B8 evidence is accepted (designed, not yet run — no
   B8 evidence exists yet, so the I8 tier order is intact). Artifact =
   per-class cursor-position table ours vs ref. {R7: G5}
B9 **Remote/hosting matrix** (RESTORED v4 — R2's §c-B8 text; v3 reused the B8
   number for width and dropped this; deep cell pass owned by P2/P3): [G]
   {ssh, mosh, tmux-over-ssh, container, CI, dumb-tty} x {echo latency under
   100ms+ RTT, redraw, TERM negotiation, missing binaries, read-only HOME,
   no /dev/pts, CI env vars changing behavior}. [F] shaped link / real remote
   env where available + I9 [M] degraded-env boot + echo matrix; cell table
   artifact. Would catch echo-lag and redraw-storm classes invisible on a
   local pty; boot failure in devcontainer. {R2: S3-1, S3-2, §c-B8; R3: §5-nonP1}
   **R7 executability rider (v6):** per B9 cell {tool, threshold,
   disposition}. Environment dispositions (measured 2026-10-17): LIVE =
   ssh (loopback sshd), tmux-over-ssh, container (docker daemon OK), CI
   env vars, dumb-tty (TERM=dumb), shaped link (tc, root: 100ms+ RTT
   latency + loss); DOCKER/RECIPE = mosh (mosh MISSING on host; container
   pair mosh-client/mosh-server over UDP). Threshold = per cell:
   boot-succeeds under the degraded env + echo-latency matrix
   (send→echo p50/p95 under each shaped RTT, ours vs ref same shaping) +
   redraw-storm ceiling (frames-per-second under a full-redraw trigger,
   ours ≤ ref ×2); read-only-HOME/no-/dev/pts cells assert the documented
   failure-or-degrade behavior, not silence. Artifact = cell table +
   latency CSV per shaped profile. {R7: G6}

## C — Behavior / State Machines
C1 Session lifecycle: [G] materialized from x to x trigger x UI-effect
   transition table (states enumerated from ref code, not prose) + named
   cells: session `fork` (`routes/session/dialog-fork.tsx`; ours: oc-server
   fork), `anchors`/`anchor-view`, `grouping/{session,tree}`,
   `message-navigation` (R3 fold 1) [F] I1 real child ids [M] table driven
   against both ends; transition-probe scripts. {R1: S5-5; R3: FOLD-1}
C2 Navigation graphs: [G] tabs/picker/family/session.child full graph incl.
   unreachable-node detection + grouping/tree navigation cells (R3 fold 1)
   [F] real child ids [M] graph walk asserts. {R3: FOLD-1}
C3 Permission+attention — **R2 §c rewrite, verbatim**: [G] decision {allow/
   deny/always} x tool x path-scope x persistence {session, forever, expiry} x
   queueing; attention: emission {bell OSC 777, OSC 9, in-process audio,
   system notify} x gating config. **V8 census pin (R9-L3):** ref's literal
   sets = Permission.Effect {allow, deny, ask} (`schema/src/permission.ts:55`)
   and Permission.Reply {once, always, reject} (`:41`) — the decision x
   persistence matrix materializes over THESE literals (allow/deny/always
   above are the behavioral buttons they compose into); census: `grep -n
   'Schema.Literals' /opt/opencode-ref/packages/schema/src/permission.ts`.
   [F] I1+I3+real grant stores [M] flow
   scripts + emission capture trace, scripted flows end-to-end.
   {R1: S1-8, S5-6; R2: (a)S5-6; R9: L3}
C4 Config leaf-key round-trips: [G] PER LEAF KEY read x write x restart x
   cross-install visibility; **JSONC comment preservation**; env overlays
   (OPENCODE_* incl. CLI_CONFIG_CONTENT); `managed-policy.ts` precedence
   interaction (U9). [F] I9 real config dirs (v2 shape, jsonc, native
   providers) [M] boot -> toggle -> restart -> v2-read round-trip assert each
   leaf. {R1: S1-12, S5-7, S3-5; R3: U9}
C5 Error/recovery paths: [G] per failure {server loss, plugin crash, partial
   write, bad data} x mid-state + workspace-file-changes review flow cell
   (`dialog-workspace-file-changes` + file-retention/file-mutation, R3 fold 6)
   [F] injected failures [M] kill/fuzz scripts + recovery assertion.
   {R3: FOLD-6}
C6 Plugin lifecycle x UI: [G] install/enable/disable/update/fail x UI x
   storage/RPC (PluginStorage, NoopRpcPort) x phase; feature-plugin SLOT
   architecture slots are surfaces (R3 fold 5). [F] I2 real plugins, real
   payloads [M] lifecycle drive + capture, **with a three-layer state
   assert per lifecycle op: (1) plugin-state API/UI cell, (2) storage
   write (PluginStorage keys), (3) RPC/registration effects — and parity
   differenced against the I2b ref-binary leg (2.0.25 + plugin); each
   layer divergence per op x phase is a finding, not a capture-only
   record**; artifact = per-op x phase state-transition table ours vs ref
   (I2b). {R1: S1-11; R2: (a)S1-11; R3: FOLD-5; R7: G12}
C7 Background jobs surface. [G] per job state x surface [F] long-running real
   job [M] async replay + observe.
   **R8-N3 rider (v7 — ratify-1 R-C):** artifact = per-job-state surface
   table + progress/lifetime assert (ours-vs-ref columns; the job's
   observable progress advances and its lifetime events — start/update/
   end — appear on the owning surface, per state). {R8: R-C}
   **V8 census pin (R9-M-3):** the job-state enumeration source is the
   tool-part status union = {streaming, running, completed, error} —
   `schema/src/session-message.ts:127/:133/:140/:148` (C1's from-ref-code-
   not-prose discipline); census: `grep -n 'status: Schema.tag'
   /opt/opencode-ref/packages/schema/src/session-message.ts`. {R9: M-3}
C8 Worktree family — **R2 §c rewrite, verbatim**: [G] {create, adopt, refresh,
   move, delete, rename} x repo state {clean, dirty, detached, bare,
   conflicted, no-git} x {git-side assertion, UI cell, session_move tool}.
   [F] I1 real repos [M] git-state assertion script per cell; op + UI + fs
   assert. **V8 census cure (R9-L4):** ref's ACTUAL worktree op set =
   {create, remove, refresh, list} (`grep -n 'Effect.fn("Worktree\.'
   /opt/opencode-ref/packages/core/src/worktree.ts` — exactly four; +
   strategy registry, default git) plus the `session_move` tool
   (`core/src/tool/plugin/opencode.ts:110` — the "move" axis) and the
   create-time name dialog (`dialog-worktree-name.tsx`); **`adopt` and
   `rename` have NO ref op** (the R2 list over-enumerated — carried
   R2→v7 uncensused; adopt/rename cells convert to ours-extra negative-
   space rows under F7, not parity cells). {R1: S1-1; R2: (a)S1-1; R9: L4}
C9 MCP family — **R2 §c rewrite, verbatim**: [G] {add, connect, auth, fail,
   restart, remove} x {dialog cells, toasts, home row, CLI} x tool invocation
   + needs-auth + elicitation-blocked. [F] I1 real MCP servers [M] live
   round-trip trace. **V8 threshold note (R9-M-3):** threshold = every cell
   completes its round-trip with matching observable state on both stacks
   (implicit in the verb, now stated). {R1: S1-2; R2: (a)S1-2; R9: M-3}
C10 Provider auth family: [G] every OAuth/device/token flow x storage x
   refresh x UI (tokens sidebar, /connect, pair + QR) [F] real provider
   sandbox creds (mock IdP + I9) [M] flow capture + credential store assert.
   {R1: S1-3; R2: (a)S1-3}
C11 Update/upgrade family — **R2 §c rewrite, verbatim**: [G] check -> notify
   -> install -> restart x method {curl, npm, pnpm, bun, yarn, vp, brew} x
   {install prefix, PATH precedence, downgrade, cache migration} + uninstall;
   update-notice cadence under I3. **V8 census pin (R9-L5):** the method
   list is EXACT — `cli/src/services/updater.ts:14` = `["curl", "npm",
   "pnpm", "bun", "yarn", "vp", "brew"]`; census: `sed -n '14p'
   /opt/opencode-ref/packages/cli/src/services/updater.ts`. [F] real updater paths (staged update)
   [M] upgrade round-trip script per method; UX + fs assert.
   {R1: S1-4, S2-9; R2: (a)S1-4; R9: L5}
C12 Onboarding/migration first-runs: [G] per first-run shape {fresh, migrated,
   mixed v2+rust install} — first-run model/provider seeding, StartupLoading
   500ms/3s gates (I3), MigrationOverlay, getting-started dismiss,
   location-missing recovery, downgrade-after-upgrade [F] synthetic v2/mixed
   homes + I9 [M] fresh-install + migrated-install scripts; observed boot +
   carried state. {R1: S1-14, S2-9, S3-7; R2: (a)S1-14}
C13 **Skills family** (v4 rewrite per R3 §2; R2 matrix restored): [G] sources
   {directory, url, embedded} x phase {discovery, permission, invocation} x UI
   (dialog-skill); embedded-vs-directory precedence; invocation-through-agent
   and URL-fetch failure paths. [F] real skill dir [M] enumeration + UI assert
   + failure-path probes. {R2: S1-1, (a)C13; R3: §2-C13}
C14 **Instructions/commands/agents/prompts discovery** (v4 rewrite; R2 proposed
   these as TWO clauses — both texts carried):
   (a) Instruction discovery: [G] AGENTS.md hierarchy (global -> project ->
   subdir) x precedence x dedup x injection order x builtins merge [F] real
   project shapes + I9 [M] **artifact = materialized final system-prompt
   diff**. {R2: S1-3, §c-C15}
   (b) Commands/agents/prompts: [G] source enum {command, agent, prompt} x
   {project, global, user, embedded} x **frontmatter-field validation
   (unknown field silently ignored = cell)** x collision x precedence x
   agent-mode binding x catalog/frecency UI [F] real `.opencode/{command,
   agent}/*.md` shapes [M] precedence assert + frontmatter validation.
   {R2: S1-2, §c-C14; R3: §2-C14}
C15 **Composer context family** (v4 rewrite per R3 §2): [G] per trigger
   {mention grammar, directory-completion, recents} x state + **payload-
   resolution cells (mention -> payload path; insert format)** + context chips
   + frecency/history/stash (IN [G], not prose) + composer tabs {shell,
   subagents, terminals} (R3 fold 2). [F] I1 real project tree [M] trigger +
   completion assert + payload-resolved effect; would catch @-mention
   resolving to the wrong path payload with a correct-looking chip.
   {R2: S1-5, (a)C15; R3: §2-C15, FOLD-2}
C16 **Long-tail dialogs** (v4 rewrite per R3 §2): [G] enumeration = filesystem
   union of THREE trees (`tui/src/component/dialog-*`, `tui/src/routes/
   session/dialog-*`, `session-ui`) — no single DialogKind symbol exists; per
   dialog x state with **ACTION-ASSERT (stash must actually stash, rename
   actually rename)**; named: dialog-stash, dialog-experiments, dialog-debug,
   dialog-status, dialog-shell-output, retry-provider, masked-text,
   command-palette, session-tabs-rail, pierre review-mode family
   (commented-lines, comment-hover, diff-selection, file-find); +
   workspace-file-changes review flow (R3 fold 6). [F] I1 [M] enumerate from
   ref trees + assert each dialog's action. **V8 census pin (R9-L6):**
   census commands = `ls -d /opt/opencode-ref/packages/tui/src/component/
   dialog-*` (23 dirs) ∪ `ls -d /opt/opencode-ref/packages/tui/src/routes/
   session/dialog-*` (4) ∪ session-ui components (counted 2026-10-17; the
   three trees are the enumeration, the named list above is illustrative
   cells within them). {R2: S1-8, (a)C16; R3: §2-C16, FOLD-6; R9: L6}
C17 **Terminal / persistent-PTY family** (U3, R3 §5 verbatim): [G] {spawn,
   resize, scroll, exit-status, handoff, daemon-restart} x {terminals-tab,
   terminal-pane, shell-output dialog, pty handlers} x TTY state. [F] I1 real
   ptys [M] session drive + capture + exit-status assert. {R3: U3}
C18 **CLI/service/run family** (U4+U5, R3 §5 verbatim): [G] every
   `cli/src/commands/handlers/*` command x args-shape x output {tty, non-tty}
   x exit code; service lifecycle {start, stop, restart, status, get, set,
   unset, registration, standalone}; `run` {interactive, noninteractive, v1} x
   stream format (headless exec: stream format, exit codes, scriptability).
   [F] I1+I10 [M] executed-command matrix artifact (every cell dispositioned).
   **V8 census pin (R9-L7):** census = `ls -1R
   /opt/opencode-ref/packages/cli/src/commands/handlers/` (top-level:
   acp, api, auth, debug, default, mini, models, pair, plugin, reload,
   run, serve, service, session/, mcp/, stats, uninstall, upgrade —
   counted 2026-10-17; nested dirs enumerate recursively).
   {R3: U4, U5; R9: L7}
C19 **Integrations family** (U6, R3 §5 verbatim): [G] per integration
   provider x {connect, disconnect, auth-state} x {dialog, picker, handlers}.
   **V8 census note (R9-L8): NO static enumeration exists** —
   `IntegrationID` is a branded string (`schema/src/integration-id.ts:3`);
   providers are runtime/plugin-registered and enumerate only via the
   `integration.list` API (consumed by `dialog-integration.tsx:85`); census
   = the list-API result on the I2b binary leg, recorded per round with
   which-ref-form per I2b. [F] I1 [M] flow assert + credential store check.
   {R3: U6; R9: L8}
C20 **Built-in tool family** (U7, R3 §5 verbatim): [G] per built-in tool x
   arg-shape x output-kind x renderer x permission class x side-effect (named:
   read-filesystem, mcp, html-markdown, http-body, runtime, plugin tools,
   websearch, file-mutation, file-access, file-retention, vcs/patch).
   **V8 census pin (R9-L9):** the tool universe enumerates from TWO dirs —
   `ls -1 /opt/opencode-ref/packages/core/src/tool/` (read-filesystem, mcp,
   html-markdown, http-body, runtime, patch.txt) ∪ `ls -1
   /opt/opencode-ref/packages/core/src/tool/plugin/` (edit, file-diff,
   glob, grep, mcp-resource, opencode, patch, question, read, shell,
   skill, subagent, webfetch, websearch, write — counted 2026-10-17; the
   named list above is illustrative, the [G] covers every census entry).
   [F] I1+I2 [M] schema+behavior+render parity vs ref per tool.
   {R3: U7; R9: L9}
C21 **Determinism/idempotency** (RESTORED v4 — R2's §c-C17 / R1 S2-13 / U11;
   v3 had zero "determin" hits): [G] rapid/duplicate input x double-submit
   guards x idempotent re-run x event-order stability under repetition.
   [F] I3 vt-clock driven [M] repetition scripts + order-stability assert;
   would catch queue double-fire on rapid Enter; duplicate session titles
   racing regeneration. {R1: S2-13; R2: S2-13, §c-C17; R3: U11}
C22 **Telemetry/observability family** (v7, NEW — R8 MG-1(a); the family
   RATIFY-1 found unowned: ref wires OTLP observability in PRODUCTION,
   unconditionally, at CLI boot — `packages/cli/src/index.ts:8` imports
   `Observability` from `@opencode/util/observability`, and
   `cli/src/index.ts:126` provides `Observability.layer({endpoint:
   process.env.OTEL_EXPORTER_OTLP_ENDPOINT, headers:
   process.env.OTEL_EXPORTER_OTLP_HEADERS, client/version/channel…})`
   into the main effect; R5's adjudication examined `packages/core` only
   and concluded "no OTel implementation" — a POST grep cannot find
   env-gated boot-layer wiring; ours has zero OTEL_*/otlp code (grep
   `crates/` = 0 code hits, 2026-10-17) — a live ref-only family
   divergence invisible to F7's ours-minus-ref letter and to any
   core-only scan; exactly the U-family class R3's sweep existed to
   catch, plus an I6-class masking premise). [G] cells =
   {env-read set, boot-layer wiring, emission gating, config-key
   behavior, always-on file-log layer} x {ours, ref}: (1) env reads —
   the ref set is exactly {OTEL_EXPORTER_OTLP_ENDPOINT,
   OTEL_EXPORTER_OTLP_HEADERS, OTEL_RESOURCE_ATTRIBUTES} (read at boot
   in `cli/src/index.ts` and in `util/src/observability{.ts,/otlp.ts}`),
   enumerated name-by-name per F7's env-read scope; malformed values
   (bad headers k=v list, bad OTEL_RESOURCE_ATTRIBUTES W3C list) are
   cells, not errors (ref parses defensively, drops bad attrs). (2) boot
   layer — provided UNCONDITIONALLY: the layer, the env reads, and
   structured logging are always on; only emission is endpoint-gated;
   OTLP-layer failure falls back to the local-only logger
   (`Layer.catchCause(() => local)`). (3) emission — OTLP JSON over HTTP
   (`OtlpSerialization.layerJson` + `FetchHttpClient.layer` +
   `OtlpExporter.layerFlusher`): logs → `{endpoint}/v1/logs`, traces →
   `{endpoint}/v1/traces` (BatchSpanProcessor; AsyncLocalStorage-
   ContextManager installed as the GLOBAL context manager), headers from
   OTEL_EXPORTER_OTLP_HEADERS parsed `k=v,k=v`; resource = serviceName
   `opencode` + serviceVersion + `deployment.environment.name` = channel
   + `opencode.client` + `opencode.run`/`service.instance.id` = runID
   (8-char UUID per process). (4) config key `openTelemetry`
   (recognized in `core/src/config/normalize.ts:49`; leaf round-trip =
   C4's; behavior disposition on/off/endpoint/headers vs ref = here).
   (5) always-on file-log layer (endpoint-independent): structured
   `key=value` lines (timestamp/level/run-id/spans/annotations) to a
   shared machine-wide append-only log file, compacted in place
   (50 MB cap → 25 MB keep, 1 h trim interval, 5 min stale-trim-lock),
   local-vs-channel naming per `Logging.loggers(local, channel)`.
   [F] I2b binary leg (ref 2.0.25 user binary — telemetry lives in the
   CLI boot path, NOT in the 2.0.24-source `packages/core`, which is
   why core-only scans kept missing it); B7's OTEL_* env cells reused;
   I9 provenance for the mock collector (instrument, never sole
   evidence). [M] mock OTLP collector (loopback HTTP recorder) as the
   standing assert target: (i) endpoint SET ⇒ after one logged action +
   one traced span, the collector holds POSTs to /v1/logs and /v1/traces
   with parsed headers + resource attrs (channel/version/client) + JSON
   body shape — ours vs ref-binary per cell; (ii) endpoint UNSET ⇒ zero
   collector traffic while the file-log layer still writes (file exists,
   structured lines carry run-id + channel); (iii) env-read trace parity
   (F7 scope: name + count + args); (iv) ours disposition AS EVIDENCED:
   zero OTel implementation = a standing divergence finding until
   dispositioned (absent-justified requires a product decision, not scan
   silence). I8 canary per the matrix's R8 design addendum (binding
   before first evidence). Artifact = per-cell parity table {env x cell x
   ours/ref-binary} + collector capture corpus. **Boundary: U8's OOC
   entry stays SERVER-SIDE `stats/` only — client observability is IN
   scope here.** {R8: MG-1}

## D — Data / Communication
D1 SSE: [G] type x field x consumer {TUI, webui} x flow x error-injection
   (publisher error mid-stream) + ordering guarantees per consumer, under
   plugin activity. [F] I1 real flows + I2 [M] subscription taps + publish->
   consume trace + field diff; I8 canary = reorder + optional-field drop.
   {R1: S5-8}
D2 REST: [G] endpoint x consumer x happy path x every documented error code x
   malformed input; **request-side field parity (what clients SEND)**.
   [F] actual client calls, live traces [M] proxy capture + shape/status
   assert. {R1: S5-9}
D3 Plugin-host boundary: [G] per API surface x real payload (omo-slim + ≥2
   other ecosystems) [F] I2 [M] host-adapter taps, boundary trace —
   **compared against the I2b ref-binary leg with a per-call disposition
   column: {parity | absent-API-on-ours (finding, F1/F7-class) |
   plugin-never-calls (justified)}; a trace with no comparison oracle is
   not evidence; an API the plugin calls on ref and cannot call on ours
   is a FINDING by definition (the vacuous-truth hazard I2b exists
   for)**. Artifact = per-surface call matrix (args + response shapes)
   with the disposition column. {R1: S3-4; R7: G13}
D4 Persistence: [G] per table/column/row-kind writer-reader pair (**pairing
   bookkeeping explicit — unpaired writers/readers must be listable**); per
   state-file key; CELL-level migration diffs; **longitudinal shapes** (state
   written by N-1 read by N, months-old rows, v1-era remnants, long-use
   session trees). [F] I9 real DB snapshots, mixed installs [M] writer/reader
   pairing lint + migration A/B + write -> read -> migrate assert.
   {R1: S5-10, S3-5; R2: M-2, (a)S5-10}
D5 Self-consistency invariants: [G] every cross-layer id/metadata chain
   (sessionID class generalized) [F] I1 [M] invariant checkers, end-to-end id
   trace.
D6 WebUI serving — **R2 §c rewrite, verbatim + U10 amendment**: [G] {asset
   hosting, SPA fallback, browser auth/pair redirect, version gating
   (incompatible-server-panel), handoff, SDK/client package shape} x real
   browser session; PLUS desktop shell (`packages/desktop/{main,preload,
   renderer,shared}`: native chrome, protocol handlers, desktop-side update)
   and app package (`packages/app`: composer/home/new-session/desktop) either
   in the served-surface enumeration or **absent-justified** (U10).
   [F] I1 [M] served-app e2e + proxy capture of the app's actual calls;
   request matrix assert. **V8 census pin (R9-L10):** `ls -1
   /opt/opencode-ref/packages/desktop/src/` = exactly {main, preload,
   renderer, shared}; `ls -1 /opt/opencode-ref/packages/app/src/` =
   {app.tsx, composer, desktop.ts, entry.tsx, home, new-session, providers,
   runtime, servers, session, settings, shell, workspaces, …} — the clause's
   named subpackages all exist (counted 2026-10-17).
   {R1: S1-7, S3-7; R2: (a)S1-7; R3: U10; R9: L10}
D7 ACP surface: [G] session/tool/**elicitation translation** granularity x
   lifecycle (per method) [F] real ACP client [M] adapter interop, session
   assert. {R1: S1-6; R2: (a)S1-6}
D8 Share/export paths: [G] publish/unpublish lifecycle x export options/
   result dialogs x CLI export formats x fork-with-export x format x
   destination [F] real data/artifacts [M] round-trip diff + output assert.
   {R1: S1-10; R2: (a)S1-10}
D9 **Model/variant/provider matrix** (v3's number KEPT; v4 rewrite per R3
   §2/§5): [G] catalog resolution x provider shapes (multi-provider configs,
   custom/local providers) x variants {effort, toggle, budget_tokens} x
   preference persistence x models-dev drift x capability flags (incl.
   supportsText/image) x dialog-model/dialog-variant UI cells — IN [G], not
   prose; **folding rule: providers grouped into classes, ≥2 providers per
   class exercised** (the unbounded hundreds-cell matrix folds); models-dev
   pinned via I10. **V8 census note (R9-L11):** the variant-kind set
   {effort, toggle, budget_tokens} is CATALOG-DATA — it lives in models-dev
   data (I10-pinned external snapshot), not in code (`schema/src/model.ts`
   defines Variant = {id + settings/headers/body overlays}, untyped kinds);
   census = the pinned models-dev snapshot, re-run on I10 re-baseline.
   [F] catalog + config variants + I10 [M] chain assert
   (config -> catalog -> dialog -> request). {R1: S3-6; R2: S1-7, (a)D9; R3: §2-D9;
   R9: L11}
D10 **Code Mode + plugin-browser** (RESTORED v4 — R2's §c-D9 / U2; v3 reused
   the D9 number for model/variant and dropped the family): [G]
   interpreter/stdlib/openapi/tool-runtime/tool-schema surface x parity;
   browser tunnel/proxy/connection/rpc/files round-trips. [F] I2 [M] per-API
   trace (stdlib divergence is invisible to D1/D2 — not server-exposed).
   {R2: S1-6, §c-D9; R3: U2}
D11 **SDK/client package** (RESTORED v4 — R2's §c-D10, silently vanished in
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

## E — Cross-cutting Quality
E1 Perf FEEL: [G] interaction × load matrix, cells = {typing, scroll, dialog
   open, resize, paste-large, paste-during-stream, search-in-10k-transcript}
   × {idle, streaming-active, 10k-msg transcript open}; frame model =
   **drive-source ticks** (a TUI has no fixed fps — "frame" = one render
   flush driven by a cadence source; pinned cadences: spinner 80ms
   `spinner.tsx`, toast animation, reconnect backoff 1s/5s — same list as
   I3). [F] ours: `frame_budget_tests.rs` + `replay.rs` (I5); ref:
   timestamped PTY capture (write-stamp deltas) under I10 pin; vt-clock per
   I3 for all cadence sources. [M] per cell: inter-flush interval histogram
   (ms) + stall-burst detector = ≥2 consecutive missed drive ticks OR any
   gap >150ms while a cadence source is active; **any stall-burst =
   finding**; artifact = per-cell histogram JSON + burst list. Canary per
   I8: injected 250ms stall must flag exactly the patched cell.
   {R1: S2-1; R2: S2-1; R5: Q1}
E2 Startup TTI: [G] boot → first-interactive per surface {tui, mini, webui,
   headless run} × config variant {plugins on, plugins off} × data size
   {fresh home, 100 sessions, 10k-msg session}; **T0 = process spawn
   (first exec), T_interactive = first frame where the composer accepts+
   echoes a key** (marker = observed echo in capture). [F] I3 vt-clock for
   gate behavior + wall-clock N=10 runs recorded as baseline artifact
   (median, p95 — same host for ours and ref, I10 pin). [M] TWO asserts:
   (a) gate BEHAVIOR — stretch boot phases under vt-clock, StartupLoading
   appears exactly when 500ms elapses and escalates at 3s (thresholds
   pinned from ref `startup-loading.tsx`: 500ms show, 3000ms gate);
   (b) baseline — ours T_interactive median ≤ ref ×1.5.
   {R1: S2-2; R2: S2-1; R5: Q2} **R7 legs amendment (v6):** the ref
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
E3 Memory growth (soak): [G] per session shape {idle session open,
   streaming session completed, 10k-msg transcript open, plugin-active}
   × ≥2 runs × ≥4h (or 1×24h); metrics = VmRSS + RssAnon + FD count
   sampled from /proc/PID every 60s (real clock — RSS is not
   virtualizable). [F] sampling harness = the F5 soak; ref baseline
   measured identically under I10 pin. [M] slope (MB/h, least-squares over
   full run) reported per run; threshold: ours slope ≤ **max(2 × ref
   slope, ref slope + 20 MB/h)**; artifact = CSV + slope table.
   Leak-canary gated per I8 before first evidence counts (see R5 §5).
   {R1: S2-3; R2: S2-1; R5: Q3}
E4 Huge-data: [G] per family {transcript render, session-list, search,
   diff view} × magnitude {1k, 10k, 100k msgs}; corpus = committed
   generator script (synthetic, I9-labeled; generation is part of the
   artifact). [F] I5 replay harness, same corpus for ours and ref (I10).
   [M] marker = **first frame containing the target content after
   load/navigate/scroll-to-message**; p95 over N=20 runs each side;
   threshold: ours p95 ≤ ref p95 ×1.5 on the same host; content
   correctness is A3's (E4 owns time only — boundary per R5 §4).
   Artifact = per-cell p95 table. {R1: S2-4; R2: S2-1; R5: Q4}
E5 Accessibility: [G] (a) contrast per theme × state × element — computed
   WCAG ratio from theme-token fg/bg pairs (post-quantizer, A7 chain);
   **threshold 4.5:1 text, 3:1 UI/large**; cells where REF itself violates
   are inherited-findings (listed, dispositioned, not parity-blocking) —
   ours must be ≥ ref ratio per cell. (b) keyboard-only path for every
   pointer target — **target enumeration = B2 hit-rect inventory ∪ C16
   dialog enumeration** (v4's "~120 auditA targets" is DELETED: `auditA`
   does not exist in ref, verified zero hits); method = scripted key walk
   reaching each target with visible focus. (c) color-independence of
   busy/error/disabled states: under grayscale transform +
   luminance-equality check (deuteranopia-class pairs share luminance),
   state pairs must remain distinguishable by non-hue channel or
   glyph/shape. [F] computed from token values (I4) [M] ratio assert +
   walk artifact + independence table; canary per I8 (R5 §5).
   {R1: S2-5; R2: S2-5; R5: Q5}
E6 i18n/locale: [G] locale classes {LANG=C, en_US.UTF-8, ru_RU.KOI8-R class,
   TZ absent} × format corpus {transcript timestamps, session-list relative
   times, date pickers} — local-tz `toLocaleString` vs fixed-UTC behavior,
   width/truncation under non-UTF-8. **Boundary: B7 owns boot/survival in
   these envs; E6 owns format/rendering correctness parity in them** (no
   duplicated boot cells). [F] real locale env (same env matrix as B7,
   reused) [M] format-diff table ours vs ref per corpus item.
   **R8-N1 rider (v7 — ratify-1 R-A):** assert = format-string equality
   per corpus item; any diff dispositioned. {R8: R-A}
   {R1: S2-6, S3-5; R2: S2-6; R5: Q6}
E7 Error-message quality: [G] error-path enumeration = union of {C5 injected
   failures, D2 documented error codes, C18 CLI exit codes}; per path:
   message copy, toast action labels, dialog copy, exit code. [F] real
   messages from the C5/D2/C18 harnesses [M] (a) **copy-diff vs ref** per
   path (objective parity); (b) placeholder scan — regex corpus
   {"Not wired in this build", TODO, FIXME, lorem} across all reachable
   surfaces, any hit in a normal flow = finding; (c) exit-code table
   completeness — every emitted code documented or dispositioned. (v4's
   "comprehensibility/actionable check" is DELETED — not agent-executable;
   comprehensibility is proxied by parity + placeholder freedom.)
   {R1: S2-10; R2: S2-10; R5: Q7}
E8 Security posture — **R2 §c rewrite, verbatim**: [G] route x auth table
   incl. static assets; pair-code entropy/expiry; credential file perms;
   traversal corpus; redaction canaries (planted secrets in logs/share/
   export); SSRF targets (fetch/websearch). [F] adversarial payloads [M]
   pen-style probe script + findings file. {R1: S2-11; R2: S5-2, (a)S2-11}
E9 Teardown/cleanup: [G] per exit path x {children, PTYs, temp files, locks,
   terminal restore}; **kill -9 recovery + panic injection**; stale locks.
   [F] kill -9/panic injection fidelity [M] post-exit system-state audit.
   **R8-N2 rider (v7 — ratify-1 R-B):** artifact = per-exit-path residue
   audit table (residue class x exit path, ours vs ref); threshold =
   zero un-dispositioned residue = pass (orphan children, live PTYs,
   temp files, held locks each dispositioned; terminal state restored).
   {R8: R-B}
   {R1: S2-12; R2: (a)S2-12}
E10 Storage/IO growth: [G] metrics = {DB main+WAL file bytes, log-dir
   bytes, state-file bytes + writes/hour (mtime count), process
   write_bytes delta /proc/PID/io} sampled every 5min during the F5/E3
   soak, per session shape; write amplification = disk-bytes-written ÷
   new-data-bytes per soak segment. [F] same soak harness as E3 (shared
   sampling loop; real clock) [M] growth-slope per metric (bytes/h);
   threshold: ours ≤ max(2 × ref baseline, ref + 50 MB/h) on identical
   session shape (I10); **unbounded monotonic growth in any metric =
   finding regardless of threshold**; artifact = CSV + slope table +
   amplification ratios. {R2: S2-2; R4 restore; R5: Q8} (v5 rewrite per
   R5 §3 — closes P1-residual nominal #1; supersedes v4's R2 §c-E10
   restore text, same gap lineage.)
E11 Idle CPU/energy: [G] idle states {no session, idle session open,
   post-stream session} × 60s windows; metrics = utime+stime delta
   /proc/PID/stat, voluntary+involuntary ctxt switches /proc/PID/schedstat
   (fallback `pidstat -w 1 60`), and **PTY output byte-rate at rest**
   (E1 capture harness reused) — animations-while-idle manifests as
   nonzero byte-rate with no cadence source active. [F] real clock,
   quiescent machine (no other load; harness asserts loadavg < 1.5), ref
   measured identically (I10) [M] per state+window: CPU% and
   wakeups/min; threshold: ours CPU% ≤ ref + 0.5%, wakeups/min ≤
   ref ×1.5, byte-rate = 0 unless a user-visible animation is active
   (each nonzero dispositioned); artifact = window table.
   {R2: S2-3; R4 restore; R5: Q9} (v5 rewrite per R5 §3 — closes
   P1-residual nominal #2; supersedes v4's R2 §c-E11 restore text, same
   gap lineage.)
E12 Search/filter family: [G] per surface {session-list filter,
   transcript/message search} × query class {ascii, case variants,
   combining chars (NFC/NFD pairs), CJK, fuzzy} — **corpus committed
   in-repo as JSON** (queries × expected-matching/non-matching strings,
   I9-labeled real-session-derived). [F] corpus + I10 pin [M]
   result-parity assert vs ref: ordered result-list equality per query;
   fuzzy = **rank-order parity** (ties dispositioned as set-equality);
   normalization audit = which of NFC/NFD/case-fold each side applies.
   Artifact = per-query diff table. {R2: S2-4; R4 new; R5: Q10} (v5
   rewrite per R5 §3; v4-new clause — R2's S2-4, v3-dropped, R4-caught —
   text superseded by R5 §3.)

## F — Adversarial / Meta
F1 Materialized command x route x state matrix. [G] every cell owned;
   exercised/unexercised bookkeeping persisted round-to-round; **per-axis
   coverage % (cells exercised / cells defined) reported per round** [F] I8
   canary on the matrix [M] cell-by-cell disposition; matrix runner + ledger
   artifact. {R1: S5-11; R2: M-4}
F2 Bottom-up ref enumeration. [G] every export in the **I10b-pinned package
   universe** accounted (ported/absorbed/absent-justified), incl. packages
   beyond tui/core; U8 stats = standing absent-justified entry [F] I10+I10b
   [M] export-diff tool; account per export. Standing accounting entries
   (R5): `tui/src/devtools` DevTools.register export — dev-only
   debug-counter groups/entries; ours absent-justified (no parity
   surface; R5-Q11). Standing accounting entry (R8, MG-1(d)):
   `util/src/observability{.ts,/}` exports (Observability.layer, Otlp
   .loggers/.tracingLayer/.resource, Logging file/compaction constants) —
   **parity-owned by C22** (env reads, boot wiring, emission gating,
   file-log layer); NOT absent-justified: ours has zero OTel code (grep
   `crates/` = 0 hits, 2026-10-17) → a live divergence held in C22's
   ledger until dispositioned there. {R2: M-4; R3: U8, U12; R5: Q11;
   R8: MG-1}
F3 Concurrency: [G] {TUI+TUI, TUI+webui} x same-session double-drive, settings
   race (flock under two clients), SSE per-client isolation, concurrent
   session create/delete, event cross-talk [F] two live clients [M]
   dual-client scripts, interleaving assert. {R1: S2-7}
F4 Network degradation: [G] per degradation {slow stream, timeout, drop,
   offline} x surface x flows; reconnect backoff (ref 1s/5s) under real drops
   (I3) [F] shaped link (tc/proxy) [M] degraded-net replay; semantics assert.
   {R1: S2-8}
F5 Soak with metrics: [G] per soak profile, hours-long real session; collects
   E1-E3, E10, E11 metrics [F] real plugins/data/clock [M] standing soak
   report.
F6 **Fresh-eyes + miss ledger + cross-axis interaction review** (v4 rewrite
   per R3 §2; cures the circularity): [G] **risk-ranked standing axis-pair
   corpus** incl. pairs with NO prior single-axis finding (precisely the class
   F6 exists for); seed pairs {theme x B6 color-depth, plugin x flow,
   concurrency x persistence, resize x streaming, mini x B6, auth x D6}; one
   designated interaction-of-the-round [F] I6 + **concrete interleaved-
   execution probe per tested pair** [M] pair-selection rule + probe artifact
   (not a prose walk). {R2: M-1; R3: §2-F6}
F7 **Negative-space parity** (v4 rewrite per R3 §2): [G] per surface x action;
   **trace scope DEFINED: {events emitted, files written, sockets opened, env
   reads, network calls}; diff granularity = name + count + args** [F]
   side-by-side traces vs ref [M] ours-minus-ref diff + **disposition rule
   (each extra = finding or justified)**; I8 canary = injected extra write +
   extra call must both trip (count+args, not name-only). Pre-registered
   dispositions (R5; openTelemetry row RE-DISPOSITIONED v7): ref-side
   `openTelemetry` config key (recognized in `config/normalize.ts`;
   **R8 correction, MG-1(c)** — the R5 wording "no implementation in ref
   core" carried a false product-level premise: implementation lives in
   `packages/util/observability`, wired unconditionally in
   `cli/src/index.ts` (boot layer; emission gated on
   OTEL_EXPORTER_OTLP_ENDPOINT); core has none; config-key leaf round-trip
   owned by C4, the telemetry behavior family owned by C22);
   event-logger writes (in F7 files/events scope, parity
   expected); stats-server traffic = U8 OOC. These rows prevent false
   positives in the ours-minus-ref network/env diff. {R2: M-3; R3: §2-F7;
   R5: Q12; R8: MG-1}

## Round log
- v1: R5 framework.
- v2: SPEC-R1 integrated (52 gaps; taxonomy-v2 [G][F][M] columns).
- v3: SPEC-R2 integrated (30 gaps; keystone [G][F][M] everywhere; A8/B8/
  C13-C16/D9/F6-interaction/F7-negative-space new; I8-I11). Integration was
  LOSSY: A9 images, C17-determinism, D10-SDK, B8-remote, E10/E11, E12-search
  dropped; several §c matrices re-compressed.
- v4: SPEC-R3 integrated **VERBATIM** (corrected integration per I11):
  restored all v3-dropped clauses (A9, C21-determinism, D10-codemode with the
  D9 number-reuse resolved — model/variant keeps D9, codemode gets D10 — D11-
  SDK, B9-remote, E10, E11, E12-search); new U-families C17-C20 + A9; D6/U10
  desktop amendment; I10b enumeration-universe pin (U12); I8 canary tiering;
  R2 §c rewrites re-applied verbatim (A3/A6/B6/B7/C3/C8/C9/C11/D6/E8); R3
  §2-driven rewrites of A8/B8/C13/C14/C15/C16/D9/F6/F7; 8 fold-amendments
  landed as named cells; traceability appendix added (102 gap IDs: R1 52,
  R2 30, R3 12+8). NEXT: SPEC-R5 (P2 quality-dimensions round; directives in
  docs/spec-round4.md).
- v5: SPEC-R5 §3 integrated **VERBATIM** (R6 integration + ratification
  round): 10 E-clause rewrites (E1 perf-feel drive-tick model +
  interaction×load matrix incl. paste-during-stream; E2 T0/T_interactive +
  two-assert gate-behavior/baseline; E3 /proc VmRSS+FD @60s with
  max(2×ref, ref+20MB/h) threshold; E4 first-paint marker N=20 ×1.5; E5
  WCAG 4.5:1/3:1 + B2∪C16 enumeration + color-independence method —
  auditA deleted, not in ref; E6 B7-boundary + format corpus; E7 copy-diff
  + placeholder scan + exit-code table; E10/E11 P1-residual nominals
  closed; E12 committed corpus + rank-order parity); 3 amendments (F2
  devtools accounting, F7 pre-dispositions, I3 spinner-80ms pin);
  appendix +12 rows → 114. Canaries (E1/E3/E5, 5 legs) + measured
  baselines + zero-loss table: docs/spec-round6.md. NEXT: P3 (adversarial
  deepening; directives in docs/spec-round6.md).
- v6: SPEC-R7 convergence round integrated (P3+P4+P5): 1 new clause (I2b
  plugin-era ground-truth binary — the RESCAN F10 vacuous-truth hazard);
  4 executability riders (B6/B7/B8/B9 — tool/threshold/disposition per
  environment, availability measured 2026-10-17); P3-D1 resolved WITH
  measurements (ref TTI both legs N=10: source-dev 5250 ms / binary 2300
  ms median; launch-path + port discipline recorded; E2 legs amendment);
  P4: I-layer audit 12/12 grounded, I8 canary-coverage matrix + E2/E12
  canary designs, I6 umbrella-masking standing rule, I5 instrument
  extension; P5: stratified-random (seed 7) 15-clause stress — rewrites
  A2/B4/C6/D3/D11; appendix +15 rows → 129. Verdict: P3/P4/P5 CLOSED →
  **FINAL candidate**; next RATIFY-1/RATIFY-2 per docs/scan-program.md.
  Report: docs/spec-round7.md.
- v7: SPEC-R8 ratification-cure round integrated (RATIFY-1 FAIL → cures
  applied per docs/ratify-1.md): MG-1 cured — new clause C22
  (telemetry/observability family: OTEL_* env-read set, unconditional
  boot layer, endpoint-gated OTLP log+trace emission, openTelemetry
  config-key behavior, always-on file-log layer; I2b binary leg;
  mock-collector assert; ref evidence `cli/src/index.ts:126`), F7
  R5-Q12 pre-disposition REWRITTEN (false "no implementation in ref"
  premise removed), B7 +OTEL_* env cells (E6↔B7-style boundary), F2
  standing accounting entry for util/observability exports, I8 canary
  matrix +C22 design; MG-2 cured — I10b pin corrected to the true
  35-package list (plugin-browser restored between merman and posts) +
  census command pinned (`ls -1 /opt/opencode-ref/packages/ | sort` /
  `| wc -l` = 35); 4 residual riders (E6 assert, E9 artifact+threshold,
  C7 artifact+assert, B3 artifact+assert — ratify-1 R-A..R-D proposed
  edits applied verbatim, tagged R8-N1..N4); N-1..N-4 registered in the
  appendix. Appendix +11 rows → 140. Verdict: **FINAL candidate 2**;
  ratification counter RESETS — RATIFY-1′ and RATIFY-2′ follow per
  docs/scan-program.md. Report: docs/spec-round8.md.
- v8: SPEC-R9 integration round (RATIFY-1′ FAIL → MG-3 cured per
  docs/ratify-1b.md): MG-3 cured — A8 REWRITTEN at census-pinned family
  granularity (mermaid kind union = exactly 6 from
  `merman/src/diagnostics.ts:1`, ALL full renderers with per-kind census
  counts; 4 non-diagram merman sub-families: fence renderer/palette/
  plugin/layout-audit; math/latex split into its TWO real renderer stacks
  — TUI `@opencode/latex` fenced {latex, math} vs session-ui KaTeX
  inline `\(...\)`/block `$$`; code-block-renderer universe census = 2
  builtin registrations; ours absent-justified per renderer, grep crates/
  = 0). Process cure — **I10b list-census guard**: every ref-enumerating
  in-clause list must cite its mechanical census command (authored sweep
  parameters exempt by class); uncited carried list = MG-3 class, fails
  ratification on sight. List-sweep (11 findings L1-L11): census pins
  added to A5/A6/C3/C11/C16/C18/C20/D6; C8 OVER-enumeration cured
  (adopt/rename have no ref op → F7 ours-extra rows); C19 named
  runtime-enumerated (integration.list, no static enum); D9 named
  catalog-data-driven (models-dev per I10). Residual riders: R-E → I10b
  standing accounting entries for function+web; R-F → I5 binds C22's mock
  OTLP collector; M-1 → R8-N6 status letter corrected to REG; M-2 → B7
  LIVE list names OTEL_*; M-3 → C7 job-state census pin + C9 threshold +
  A4 clock note. Appendix +17 rows → 157 (R9 section, tags R9-#).
  Verdict: **FINAL candidate 3**; ratification counter RESETS —
  RATIFY-1″ and RATIFY-2″ follow per docs/scan-program.md. Report:
  docs/spec-round9.md.

---

## Appendix — Gap traceability index (zero-loss ledger)

Every gap ID ever emitted by a round maps to exactly one destination below.
Status legend: **C** = owning clause (text in this file) · **A** = amendment/
named cell folded into an existing clause · **OOC** = out-of-scope with
justification (I10b) · **REG** = registered residual (non-blocking; carried
in the ledger, no clause change). Counts: R1 52/52 · R2 30/30 · R3 20/20
· R5 12/12 · R7 15/15 · R8 11/11 · R9 17/17 = **157/157**.

### R1 gaps (52)

| Gap | Destination | Status |
|---|---|---|
| S1-1 worktree | C8 (R2 §c rewrite) | C |
| S1-2 MCP | C9 (R2 §c rewrite) | C |
| S1-3 provider auth | C10 | C |
| S1-4 update/upgrade | C11 (R2 §c rewrite) | C |
| S1-5 mini runtime | A6 (R2 §c rewrite) | C |
| S1-6 ACP | D7 | C |
| S1-7 WebUI app | D6 (R2 §c rewrite + U10) | C |
| S1-8 sound/notify | C3 (emission semantics) | C |
| S1-9 images | **A9 (restored v4)** | C |
| S1-10 export/share | D8 | C |
| S1-11 plugin ecosystem | A5 + C6 + D3 + I2 | C |
| S1-12 keymap+config write | B1 + C4 | C |
| S1-13 theme system | A7 | C |
| S1-14 onboarding | C12 | C |
| S2-1 perf feel | E1 (metric+detector) | C |
| S2-2 startup TTI | E2 (gates+variants) | C |
| S2-3 memory growth | E3 (slope+cadence) | C |
| S2-4 huge data | E4 + A3 | C |
| S2-5 accessibility | E5 (color-independence) | C |
| S2-6 CJK/i18n | E6 (+ E12 search) | C |
| S2-7 concurrency | F3 | C |
| S2-8 network degradation | F4 | C |
| S2-9 upgrade/migration UX | C11 + C12 + D4 | C |
| S2-10 error quality | E7 | C |
| S2-11 security posture | E8 (R2 §c rewrite) | C |
| S2-12 teardown | E9 | C |
| S2-13 determinism | **C21 (restored v4)** | C |
| S3-1 terminal matrix | B6 (R2 §c rewrite) | C |
| S3-2 platform matrix | B7 (R2 §c rewrite) | C |
| S3-3 shell/env matrix | B7 + C4 | C |
| S3-4 plugin ecosystem matrix | I2 (≥3 ecosystems + marketplace) + A5 + D3 | C |
| S3-5 data-shape matrices | A3 + C4 + D4 + E6 (+ I9) | C |
| S3-6 provider/model matrix | D9 | C |
| S3-7 cross-version interop | C12 + D4 + D6 | C |
| S4-1 standing instrument | I-layer header | C |
| S4-2 real-payload provenance | I1 + I9 | C |
| S4-3 ref+plugin pairing | I2 | C |
| S4-4 temporal beyond animation | I3 (amended v4) + A4 + B1 | C |
| S4-5 fidelity all axes | I-layer + per-clause [G][F][M] | C |
| S4-6 miss-attribution ledger | I6 | C |
| S4-7 process + falsifiability | I5 + I8 + I11 | C |
| S5-1 token x cell x state | A1 (state union restored) | C |
| S5-2 numeric sweeps | A3 (R2 §c rewrite) | C |
| S5-3 binding x route x mode x layer x config | B1 | C |
| S5-4 target x state x shape x lifetime | B2 | C |
| S5-5 materialized transition table | C1 | C |
| S5-6 permission granularity | C3 | C |
| S5-7 config leaf granularity | C4 (JSONC + restart + cross-install) | C |
| S5-8 SSE field x flow x error | D1 | C |
| S5-9 REST negative paths | D2 | C |
| S5-10 persistence pairing | D4 (bookkeeping explicit) | C |
| S5-11 materialized matrix | F1 | C |

### R2 gaps (30)

| Gap | Destination | Status |
|---|---|---|
| S1-1 skills | C13 (matrix restored v4) | C |
| S1-2 commands/agents/prompts | C14(b) | C |
| S1-3 instruction discovery | C14(a) (system-prompt-diff artifact) | C |
| S1-4 rich content | A8 (rewritten v4) | C |
| S1-5 composer context | C15 (rewritten v4) | C |
| S1-6 Code Mode/plugin-browser | **D10 (restored v4)** | C |
| S1-7 model/variant UI | D9 (rewritten v4) | C |
| S1-8 long-tail dialogs | C16 (rewritten v4) | C |
| S2-1 E1-E4 unfalsifiable | E1/E2/E3/E4 (metric+threshold+artifact) | C |
| S2-2 storage/IO growth | **E10 (restored v4)** | C |
| S2-3 idle CPU/energy | **E11 (restored v4)** | C |
| S2-4 search/filter quality | **E12 (NEW v4 — v3 drop, R4-caught)** | C |
| S2-5 selection/copy | B2 (rendered-content round-trip) | A |
| S3-1 remote SSH/mosh | **B9 (restored v4)** | C |
| S3-2 container/CI | B9 | C |
| S3-3 width implementation | B8 (rewritten v4) | C |
| S3-4 Windows/ConPTY | B7 | C |
| S3-5 locale matrix | B7 + E6 | C |
| S4-1 [G][F][M] keystone | whole-spec format + clause ID tags | C |
| S4-2 falsifiability canary | I8 (tiered v4) | C |
| S4-3 fixture provenance | I9 | C |
| S4-4 ground-truth pinning | I10 | C |
| S4-5 output contract/no-summary | I11 (this v4 = corrected instance) | C |
| S4-6 in-repo instruments | I5 (bound v4) | A |
| S5-1 10/10 inventory-grade | R2 §c rewrites applied verbatim (A3 A6 B6 B7 C3 C8 C9 C11 D6 E8) | C |
| S5-2 E8 probe set | E8 | C |
| M-1 cross-axis interactions | F6 (rewritten v4) | C |
| M-2 longitudinal data-shape | D4 | A |
| M-3 negative-space parity | F7 (rewritten v4) | C |
| M-4 change-management + coverage | clause gap-ID tags + appendix + F1 coverage % | C |

### R3 register (12 families + 8 folds)

| ID | Family / fold | Destination | Status |
|---|---|---|---|
| U1 | images | **A9** (restored) | C |
| U2 | Code Mode + plugin-browser (package pin restored R8-MG2) | **D10** (D9 number-reuse resolved) | C |
| U3 | terminal/persistent-PTY | **C17** | C |
| U4 | CLI command surface | **C18** | C |
| U5 | noninteractive/run mode | C18 (named cell-family) | A |
| U6 | integrations | **C19** | C |
| U7 | built-in tool family | **C20** | C |
| U8 | stats/telemetry (server-side `stats/` only; client observability → C22 per R8-MG1) | F2/I7 absent-justified entry + I10b OOC list | OOC |
| U9 | account/identity/enterprise | I10b OOC list + C4 managed-policy note | OOC |
| U10 | desktop + app frontends | D6 amendment (named or absent-justified) | A |
| U11 | determinism/idempotency | **C21** (restored) | C |
| U12 | enumeration-universe pin | **I10b** | C |
| FOLD-1 | fork/anchors/grouping/message-navigation | C1 + C2 named cells | A |
| FOLD-2 | composer tabs shell/subagents/terminals | C15 | A |
| FOLD-3 | thinking-syntax.ts | A8 family list | A |
| FOLD-4 | ssh-askpass.ts | B7 | A |
| FOLD-5 | feature-plugin slot architecture | C6 + A5 | A |
| FOLD-6 | file-retention/mutation + workspace-file-changes | C16 + C5 | A |
| FOLD-7 | editor-zed* | B7 | A |
| FOLD-8 | attention-sounds.{bun,node} split | B7 | A |

### R5 gaps (12)

| Gap | Destination | Status |
|---|---|---|
| R5-Q1 | E1 interaction×load matrix, drive-tick frame model, paste-during-stream cell | E1 (v5 rewrite) | C |
| R5-Q2 | E2 T0/T_interactive markers, gate-behavior assert + baseline artifact | E2 (v5 rewrite) | C |
| R5-Q3 | E3 threshold value + /proc sampling method (V mRSS+FD @60s) | E3 (v5 rewrite) | C |
| R5-Q4 | E4 first-paint marker, N=20, ×1.5 threshold | E4 (v5 rewrite) | C |
| R5-Q5 | E5 WCAG 4.5:1/3:1, B2∪C16 target enumeration, color-independence method | E5 (v5 rewrite) | C |
| R5-Q6 | E6/B7 boundary + format corpus | E6 (v5 rewrite) | C |
| R5-Q7 | E7 copy-diff + placeholder scan + exit-code table (subjective check dropped) | E7 (v5 rewrite) | C |
| R5-Q8 | E10 nominal #1 closure (metrics @5min on F5 soak, slope artifact) | E10 (v5 rewrite) | C |
| R5-Q9 | E11 nominal #2 closure (schedstat/pidstat @60s windows, byte-rate=0) | E11 (v5 rewrite) | C |
| R5-Q10 | E12 committed corpus JSON + rank-order fuzzy parity | E12 (v5 rewrite) | C |
| R5-Q11 | devtools DevTools.register observability groups | F2 standing accounting entry | A |
| R5-Q12 | F7 ref-side-emission pre-dispositions (openTelemetry, event-logger, stats) — openTelemetry leg re-dispositioned by R8-MG1-F7 | F7 amendment | A |

### R7 gaps (15)

| Gap | Destination | Status |
|---|---|---|
| R7-G1 | plugin-era ground truth = ref 2.0.25 binary + plugin (2.0.24 cannot load plugins; vacuous-truth hazard, RESCAN F10) | **I2b** (new clause) | C |
| R7-G2 | ref-runtime provisioning resolved: launch-path (packages/cli, tsconfig-by-CWD) + managed-service port/detached-server discipline | I2b (method) + E2 (legs) | A |
| R7-G3 | B6 executability: per-cell tool/threshold/environment/disposition table | B6 rider | A |
| R7-G4 | B7 executability: env(1) boot+smoke runner, per-cell dispositions incl. ConPTY/macOS recipes | B7 rider | A |
| R7-G5 | B8 executability: corpus runner tool + zero-desync threshold + canary-before-evidence | B8 rider | A |
| R7-G6 | B9 executability: per-cell dispositions (ssh/tmux/docker/tc LIVE; mosh recipe) + echo-latency + redraw-storm thresholds | B9 rider | A |
| R7-G7 | umbrella-adjudication masking standing rule (RESCAN §4; any-instrument scope) | I6 rider | A |
| R7-G8 | standing-instrument list extension (R6/R7 canary + TTI + E12 + rescan-tui + spec-p3 probes) | I5 rider | A |
| R7-G9 | canary-coverage matrix (RUN/DESIGNED/NONE-DUE/OVERDUE) + E2 delay-injection + E12 corpus-poisoning designs + defer-or-design rule | I8 rider | A |
| R7-G10 | A2 threshold/artifact (zero unexpected cell diffs; expected-class disposition) | A2 (v6 rewrite) | C |
| R7-G11 | B4 threshold/artifact (byte-identical round-trip + normalization-class list + OSC-52/IME asserts) | B4 (v6 rewrite) | C |
| R7-G12 | C6 three-layer state assert + I2b parity target | C6 (v6 rewrite) | C |
| R7-G13 | D3 comparison oracle + per-call disposition column (absent-API = finding) | D3 (v6 rewrite) | C |
| R7-G14 | D11 tool/threshold/artifact (type-level export diff, zero un-dispositioned breaking) | D11 (v6 rewrite) | C |
| R7-G15 | E2 two ref legs (source-dev + binary), ratio gates on binary leg, measured N=10 both | E2 rider | A |

### R8 gaps (11 — RATIFY-1 register, cures per docs/ratify-1.md)

| Gap | Destination | Status |
|---|---|---|
| R8-MG1 | telemetry/observability family unowned: ref wires OTLP Observability unconditionally at CLI boot (`cli/src/index.ts:126`, OTEL_* env reads); ours zero OTel — live divergence invisible | **C22** (new clause) + B7 OTEL_* cells + F2 accounting entry + I8 canary design | C |
| R8-MG1-F7 | F7's R5-Q12 pre-disposition inherited the false "no implementation in ref" premise (core-only adjudication) | F7 pre-disposition REWRITTEN (v7, MG-1(c)) | A |
| R8-MG2 | I10b pin claimed 35 packages, enumerated 34 — `plugin-browser` (D10-owned) missing from the F2/I7 completeness backstop | I10b pin corrected + census command pinned (v7) | A |
| R8-N1 (=R-A) | E6 artifact without assert/threshold | E6 rider (v7): format-string equality per corpus item; any diff dispositioned | A |
| R8-N2 (=R-B) | E9 threshold/artifact implicit | E9 rider (v7): residue audit table + zero un-dispositioned residue = pass | A |
| R8-N3 (=R-C) | C7 thinnest clause — no artifact/threshold, "observe" verb only | C7 rider (v7): per-job-state surface table + progress/lifetime assert | A |
| R8-N4 (=R-D) | B3 no named artifact/threshold | B3 rider (v7): per-stack routed-input trace table + routing-matches-ref assert | A |
| R8-N5 (=N-1) | B7 linux-as-live-OS implicit; B9 TERM-negotiation/missing-binaries thresholds ride boot+echo | registered — cosmetic (S3 verdict CLEAN); rides stated cells | REG |
| R8-N6 (=N-2) | F7 ours-minus-ref direction structurally blind to ref-only families (the MG-1 mechanism) | cured structurally by R8-MG1 (family now owned bottom-up in C22); F7 direction unchanged (symmetric diff would drown in ref-dev noise). **Status letter corrected v8 (R9-M-1): was "A", honestly REG** — this row records a decision; the structural cure itself lives in R8-MG1's row/destination | REG |
| R8-N7 (=N-3) | F1 coverage ledger has no aggregate closure condition | registered — round structure governed by docs/scan-program.md; per-axis % per round stands | REG |
| R8-N8 (=N-4) | gui-extensions (806 files) owned transitively via D6/U10 + F2 — transitivity implicit | registered — D6(U10) letter forces the app disposition; F2/I10b family accounting explicit (v7 census) | REG |

### R9 gaps (17 — RATIFY-1′ register per docs/ratify-1b.md + the v8 list-sweep; cures per docs/spec-round9.md)

| Gap | Destination | Status |
|---|---|---|
| R9-MG3 | A8's carried enumeration lists contradicted ref source: mermaid sub-families 4 of 6 (`state`/`timeline` missing; union ground truth `merman/src/diagnostics.ts:1`, all 6 full renderers) AND math/latex "inline vs block" mis-attributed (TUI latex = fenced {latex, math} only; inline/display is session-ui KaTeX via `ui/src/context/marked-parser.tsx`) — the MG-2/R2→v7 verbatim-carry class inside a clause | **A8 v8 rewrite** (complete merman+latex census: 6 kinds x full-renderer status + 4 non-diagram merman sub-families + both math stacks + code-block-renderer universe census + ours absent-justified per renderer) + **I10b list-census guard** (the process cure) | C |
| R9-L1 | A6 mini panel list valid but carried without census citation | A6 v8 census pin (`ls -1 …/tui/src/mini/ | grep -E …` — 12 panels verified) | A |
| R9-L2 | A5 feature-plugin slot list valid but uncited | A5 v8 census pin (`ls -1 …/feature-plugins/` = exactly {home, prompt, sidebar, system}) | A |
| R9-L3 | C3 permission decision/persistence sets carried without literal census | C3 v8 census pin (Effect {allow,deny,ask} `permission.ts:55`; Reply {once,always,reject} `:41`) | A |
| R9-L4 | C8 worktree op list OVER-enumerates ref: {adopt, rename} have no ref op (census: `Effect.fn("Worktree…"` = {create, remove, refresh, list} + session_move + name dialog) | C8 v8 census cure (adopt/rename convert to F7 ours-extra rows, not parity cells) | A |
| R9-L5 | C11 updater method list carried uncited (happens to be exact) | C11 v8 census pin (`updater.ts:14` — 7 methods exact) | A |
| R9-L6 | C16 dialog enumeration trees named but no census command | C16 v8 census pin (component/dialog-* 23 ∪ routes/session/dialog-* 4 ∪ session-ui) | A |
| R9-L7 | C18 handler enumeration source named but no census command | C18 v8 census pin (`ls -1R …/cli/src/commands/handlers/`) | A |
| R9-L8 | C19 "per integration provider" — NO static enumeration exists in ref (runtime/plugin-registered) | C19 v8 census note (runtime `integration.list` API on the I2b leg; recorded per round) | A |
| R9-L9 | C20 named tool list uncited; [G] "per built-in tool" lacked enumeration source | C20 v8 census pin (core/src/tool/ ∪ core/src/tool/plugin/ = 6 + 15 entries) | A |
| R9-L10 | D6 desktop subpackage list uncited | D6 v8 census pin (desktop/src = {main, preload, renderer, shared} exact) | A |
| R9-L11 | D9 variant-kind set {effort, toggle, budget_tokens} carried as if code-enumerable | D9 v8 census note (catalog-data: models-dev snapshot per I10; schema Variant is untyped overlays) | A |
| R9-R-E | hosted share pair `function` (CF-worker backend) + `web` (viewer/docs) had no standing accounting entry (ratify-1b residual) | I10b v8 standing entries (function = hosted share backend, no client in pinned source/2.0.25 binary; web = viewer riding D8 destination cells) | A |
| R9-R-F | C22's mock OTLP collector clause-declared standing but not I5-named (ratify-1b residual) | I5 v8 extension (named; committed in-tree at first C22 execution) | A |
| R9-M-1 | R8-N6 appendix status letter "A" mislabeled a decision record as an amendment (ratify-1b cosmetic) | appendix row corrected v8 (A → REG, note in place) | REG |
| R9-M-2 | B7 v6 LIVE list didn't repeat OTEL_* though the R8 rider extends the set (cosmetic inconsistency) | B7 v8 LIVE-list addition (OTEL_* named, tagged) | A |
| R9-M-3 | C7 job-state source unnamed; C9 threshold implicit; A4 ref-side clocking implicit (ratify-1b cosmetics) | C7 v8 census pin (tool-part status union, session-message.ts:127-148) + C9 v8 threshold note + A4 v8 clock note | A |

R3 §2 clause-stress verdicts (9 rows) are cured by the v4 rewrites of
A8/B8/C13/C14/C15/C16/D9/F6/F7 — each rewritten clause carries its R3 §2
provenance tag. R3 §3's five canary designs are bound under I8.
