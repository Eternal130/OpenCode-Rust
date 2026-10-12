# TUI side-by-side differential — DECISION-GRADE mismatch report

Lane `lane/diff-rt` (isolated worktree `/tmp/opencode/lane-diff`, one
commit). Rig: `tools/parity/src/tui_diff.rs` + `src/bin/tui-diff.rs`
+ `tests/tui_diff.rs` + the JSON corpus in `tools/parity/
tui-diff/scripts/` (architecture in `README.md`).

**The bar**: v2.0.24's TUI and our Rust TUI run in the SAME project
directory (identical seeded git repo + files; one shared
`opencode.json` pointing BOTH stacks at ONE shared SSE stub,
`parityfake/fake-model`), own HOME/data roots per side (DBs cannot be
shared), IDENTICAL input bytes (keys / text / SGR mouse / leader
chords / resizes) with identical pacing, and the COMPLETE cell grid
(char + style digest) compared after EVERY step.

## Run provenance

- Date: 2026-10-11. Corpus: 14 scripts × 2 sizes (80x24, 120x30) × 2
  runs = 56 executions, 0 driver errors, 56/56 boots on both sides
  (`/tmp/opencode/tui-diff/corpus/report.md`, log `corpus-run.log`).
  `mouse-hover-tabstrip` was rerun separately (`/tmp/opencode/tui-
  diff/hover/`) after fixing its hover row (strip is grid row 0);
  its rerun rows replace the original ones below.
- Ref runtime: v2.0.24 checkout `/tmp/opencode/ref` (self-healing
  path verified by the parity unit suite; checkout pre-existed with
  node_modules, so no live rebuild was needed this run). Bun pinned
  per-process via `PARITY_BUN=/tmp/opencode/bun/bin/bun` (1.3.10 —
  the same binary all probes used; the ref pins 1.4.2 as
  packageManager but 1.3.10 runs it fine).
- Gates: cargo through `tools/dev-gate.sh` (CPU/memory caps); bun
  runs direct, as mandated. Gated nextest entry verified green:
  `PARITY_BUN=… OC_TUI_DIFF=1 cargo nextest run --run-ignored only
  -E 'test(tui_diff)'` → PASS in 35 s (boots both TUIs, asserts rig
  execution only).

## Corpus results (steps aggregated over both runs)

| script | size | steps | identical | text-only | mismatch |
|---|---|---|---|---|---|
| boot-home | 80x24 | 6 | 0 | 0 | 6 |
| boot-home | 120x30 | 6 | 0 | 0 | 6 |
| composer-clear | 80x24 | 20 | 0 | 0 | 20 |
| composer-clear | 120x30 | 20 | 0 | 0 | 20 |
| help-dialog | 80x24 | 16 | 4 | 0 | 12 |
| help-dialog | 120x30 | 16 | 4 | 0 | 12 |
| long-reply-scroll | 80x24 | 32 | 0 | 0 | 32 |
| long-reply-scroll | 120x30 | 32 | 0 | 0 | 32 |
| mouse-hover-tabstrip (rerun) | 80x24 | 34 | 0 | 0 | 34 |
| mouse-hover-tabstrip (rerun) | 120x30 | 34 | 0 | 0 | 34 |
| palette | 80x24 | 12 | 0 | 0 | 12 |
| palette | 120x30 | 12 | 0 | 0 | 12 |
| rename-dialog | 80x24 | 30 | 0 | 0 | 30 |
| rename-dialog | 120x30 | 30 | 0 | 0 | 30 |
| resize | 80x24 | 12 | 0 | 0 | 12 |
| resize | 120x30 | 12 | 0 | 0 | 12 |
| session-list-dialog | 80x24 | 28 | 0 | 0 | 28 |
| session-list-dialog | 120x30 | 28 | 0 | 0 | 28 |
| tabs-keyboard | 80x24 | 16 | 0 | 0 | 16 |
| tabs-keyboard | 120x30 | 16 | 0 | 0 | 16 |
| themes-nord | 80x24 | 16 | 0 | 0 | 16 |
| themes-nord | 120x30 | 16 | 0 | 0 | 16 |
| thought-expand | 80x24 | 28 | 0 | 0 | 28 |
| thought-expand | 120x30 | 28 | 0 | 0 | 28 |
| thought-render | 80x24 | 22 | 0 | 0 | 22 |
| thought-render | 120x30 | 22 | 0 | 0 | 22 |
| type-edit-submit | 80x24 | 26 | 0 | 0 | 26 |
| type-edit-submit | 120x30 | 26 | 0 | 0 | 26 |
| **TOTAL** | — | **656** | **8** | **0** | **648** |

- The 8 identical steps are help-dialog's post-exit blanks: v2 exits
  on `ctrl+c` with an empty prompt, and so do we — teardown parity.
- **Determinism split (mandated double run): every single mismatch is
  deterministic** — identical mismatch counts and first-diff cells in
  run 0 and run 1 for all 56 executions. No step-level timing class
  (b) exists in this corpus; both stacks render deterministically
  under the rig's settle policy.
- 0 text-only steps: every step's divergences include geometry or
  chrome glyph differences the structural filter correctly refuses to
  mask (missing rows, different chrome text), so structural mode
  currently classifies nothing as pure model-text drift. Once the
  missing-row findings below are fixed, the transcript steps are
  expected to collapse to text-only/identical (the transcript glyph
  geometry already matches — see the parity positives).

## Mismatch backlog (classified)

Class: **(a)** definite bug (ours wrong) · **(b)** ref-side
nondeterminism (timing/random content) · **(c)** legitimate
registered divergence (none found in-frame; even the version banners
match at 2.0.24).

### S0 — global style emission (touches every cell of every frame)

1. **(a) SGR color encoding — FIXED (lane/s0-color)**: v2's raster
   (`@opentui/core-linux-x64@0.5.14` `libopentui.so`) degrades RGB
   to 256-INDEX SGR at emission time when the terminal's capability
   probe says 256-color; we emitted TRUECOLOR regardless of TERM.
   Replicated in `crates/oc-tui/src/render/color_mode.rs` (decision
   matrix + nearest-palette quantization, sweep-verified 144/144
   against the ref raster) and wired at the ratatui draw seam in
   `app.rs` (`App::frame()` and the goldens stay truecolor — v2's
   own split). See the S0 notes at the bottom of this report.
2. **(a) theme value drift — PHANTOM, closed as part of 1**: the
   theme assets are byte-identical
   (`crates/oc-theme/assets/v2/opencode.json` ≡
   `packages/tui/src/theme/assets/v2/opencode.json` in the ref) and
   the resolved tokens are golden-pinned to the `@opencode/theme`
   TS pipeline (`crates/oc-theme/tests/goldens/opencode.{dark,
   light}.json`): bg resolves to `#0a0a0a` on BOTH sides. The
   reported `#080808` was v2's EMITTED `48;5;232` canonicalized
   back through the xterm-256 palette — a pure shadow of finding 1.

### S1 — boot/home chrome (boot-home, every script's step 0)

3. **(a) boot model selection ignores the config default**: with
   `OPENCODE_CONFIG` pinning `parityfake/fake-model` (the ref footer
   shows `Build · Fake Model parityfake`), our footer shows
   `z-ai/glm-5.3-flash-apex` — a bundled-catalog model — and an
   unprompted submit fails with `NANO_GPT_API_KEY is not set`
   (reproduced outside the rig: the server's `/api/config` DOES
   report the pinned default; the TUI-side initial selection is what
   diverges).
4. **(a) footer left slot**: v2 renders `cwd:git-branch`
   (`/…/project:master`) + key hints; we render the model name there
   and no project/branch.
5. **(a) composer model row missing**: v2 draws a model line inside
   the composer block (`┃ Build · Fake Model parityfake`); we leave
   that composer row blank (one extra blank `┃` row instead).
6. **(a) placeholder pool**: v2 rotates a pool per boot
   (`"Fix broken tests"`, `"What is the tech stack of this
   project?"`, …); ours is constant (`"Fix a TODO in the codebase"`).
   The pool contents AND rotation both diverge; even with our pool
   fixed to v2's, the per-boot pick is **(b)** random on the ref side
   (two processes cannot sync the pick) — EXACT mode will always
   mismatch this line; the STRUCT glyph mask already absorbs it.

### S2 — /models dialog (all prompt scripts, steps `models-*`)

7. **(a) dialog title**: ref `Select model`, ours `Switch model`.
8. **(a) filter echo clipping**: typing `fake` shows `fake` on ref,
   `fak` on ours (our dialog input clips the last glyph).
9. **(a) list source**: with the identical config, ref lists the one
   configured model (`● Fake Model` + `View all integrations
   ctrl+a` hint); we list fuzzy matches from the whole bundled
   catalog (`Step 3.7 Flash Thinking`, `Qwen3.7 …`, `GLM 4.7 …`,
   NanoGPT/302.AI provider column) — the same root cause as S1#3.
10. **(a) 80-col overflow in our dialog**: provider-column rows run
    into the right border (`gemini-3.5-flash-thinking 302.AI ▀▀▀▀`),
    `commands` wraps alone.
11. **(a) variant sub-dialog missing**: selecting a model on ref
    opens `Select variant` (Default/low/medium/high); we close
    straight back to home. (Scripts dismiss it with `esc` to stay
    comparable.)

### S3 — transcript + session chrome (type-edit-submit, thought-*,
long-reply-scroll, rename-dialog, mouse-hover)

12. **(a) session auto-title**: ref titles the tab from the reply
    (`Parity fake says hello.`); ours shows `Untitled session`
    (downstream: the tab-title width shifts the ✕/+ affordance
    columns — see #18).
13. **(a) reply stats row missing**: ref renders
    `Build · Fake Model · 134ms · 291.7 tok/s` under the reply; we
    render nothing. (The `ms`/`tok/s` values themselves are **(b)**
    per-run; the missing row is (a).)
14. **(a) thought duration suffix missing**: ref `+ Thought · 8ms`,
    ours `+ Thought`. (Value (b); missing suffix (a).)
15. **(a) transcript scroll keys**: on the identical 32-line reply,
    ref `pageup` reveals earlier rows (text-rows 17→19); ours goes
    18→14 — our paged scroll moves a different amount (or direction);
    final `end` states also differ (17 vs 18 rows visible).
16. **(a) session list (leader+l) no-op on our side**: ref opens the
    sessions browser (Subagents / Shell / Terminals tabs, `esc` to
    close); our screen does not change at all through
    leader+l/down/enter.
17. **(a) rename dialog (ctrl+r)**: renders on both (geometry
    differs: ref 9 text-rows vs our 6 — ref's dialog is taller) and
    the post-rename tab title differs via #12.
18. **PARITY POSITIVE — tab hover lifecycle**: hovering the tab
    title reveals the `✕` close affordance on BOTH stacks; moving
    off-strip hides it on BOTH (verified step-by-step in the rerun;
    the only hover diff is the title text from #12). Mouse SGR
    motion handling matches.

### S4 — layout at other sizes (resize)

19. **(a) vertical centering off by one**: at 60x20 the ref wordmark
    block starts one row higher than ours (5 vs 6).
20. **(a) footer overflow at narrow widths**: at 60 cols our footer
    renders `z-ai/glm-5.3-flash-apexshift+tab agents…` — model string
    collides with the hints with no separator/truncation.
21. **(a) resize handling otherwise works on both** (both reflow at
    100x28 and 60x20; grids re-capture cleanly) — PARITY POSITIVE
    apart from #19/#20.

### S5 — dialogs elsewhere

22. **(a) help dialog (/help)**: renders on both; text-row counts
    differ (ref 7 vs our 6 at 80 cols) — ref's dialog is one row
    taller (same family as #17).
23. **(a) palette (ctrl+p)**: opens/closes on both; ref text-rows 10
    vs our 9 — one row taller again. Browse keys work on both.
24. **(a) themes (/themes + nord)**: dialog opens, filter and apply
    run on both; global styles still diverge per S0, so nord-value
    equality is unverifiable until S0 lands (follow-up gate).
25. **(a) leader+n / alt+down / alt+up / leader+w (tabs-keyboard)**:
    NO grid change on EITHER side at home without sessions — the
    corpus cannot observe tab ops from a sessionless home. Script
    gap (phase-2): seed a session before the tab chord steps.

### Corpus phase-2 items (disproportionate now, per the mandate)

- Multi-session/long-RESTORED transcripts (cross-stack DB seeding).
- Tab chords observable only with live sessions (#25).
- Theme-value verification blocked on S0.

## Normalizations (complete, justified)

1. **STRUCT glyph mask** — alphanumeric → `x`. Rescues only glyph
   identity of run-variable text (placeholder pool #6, timing values
   #13/#14); never position/extent/punctuation.
2. **STRUCT color canonicalization** — indexed colors resolve via
   the xterm-256 palette to RGB, then ±8/channel perceptual
   tolerance. Introduced to see past the emitter-encoding difference
   (#1, every cell) before it was fixed; after lane/s0-color both
   stacks emit the SAME encoding under the rig's TERM, so this now
   only guards against future encoding regressions. EXACT mode
   keeps colors byte-strict.
3. **Cursor position** is never compared (grids carry cell contents
   only). Nothing else is normalized; in particular NO path/date/
   timing masking was needed (paths are identical by construction —
   same project dir; the ref's `cwd:branch` footer matches because
   both run in the same seeded repo).

## Gates

- Corpus: `PARITY_BUN=/tmp/opencode/bun/bin/bun tools/dev-gate.sh
  'cargo run -p parity --bin tui-diff -- --runs 2'` (dev-gate caps
  cargo; bun direct).
- Per-PR-shaped gate: `PARITY_BUN=… OC_TUI_DIFF=1 cargo nextest run
  --run-ignored only -E 'test(tui_diff)'` (skips without bun/ref —
  OC_TUI_SMOKE pattern; PASS verified, 35 s).
- `--strict` exists for CI once the backlog burns down (exit 1 on
  driver error or any MISMATCH step). CI plan documented in
  `tools/RELEASE.md` (ref-checkout + tui-diff jobs, nightly cadence;
  not wired).

## Fix-wave ordering recommendation

S0 first (it blocks all style verification and the STRUCT pass
rate), then S1#3/#4/#5 (boot chrome), S2 (models dialog incl. the
catalog-vs-config list source), S3 #12/#13 (titles/stats), then the
per-dialog row-height family (#17/#22/#23), then scroll (#15).
#18/#21 and the transcript/thought geometry are already at parity —
protect them with regression expectations once S0 lands.

---

## S0 fix-wave results (lane/s0-color, 2026-10-11)

### Where v2's emitter decides color encoding (citations)

* **Capability decision — native raster, not JS.** The v2 TUI passes
  no color options (`packages/tui/src/app.tsx:236-253`
  `CliRendererConfig`); the decision lives in the compiled zig raster
  `@opentui/core-linux-x64@0.5.14` (`libopentui.so`), whose JS-facing
  surface is `TerminalCapabilities { rgb, ansi256 }`
  (`@opentui/core/dist/types.d.ts:63-86`). Disassembly of the
  capability probe: `COLORTERM` == `truecolor`/`24bit` → rgb+ansi256
  (strings at .rodata 0x4db71/0x4cde8, compare block at text
  0x32b9e7); `WT_SESSION` → same (0x32ba5b); `TERM` containing
  `256color` → ansi256 only (0x32aede). Empirically completed (PTY
  probes against the ref): `SSH_CONNECTION`/`SSH_CLIENT`/`SSH_TTY`
  (any one) ALSO force truecolor; plain `xterm`/`screen`/`dumb` and
  unset TERM stay TRUECOLOR — the raster has no 16-color mode.
* **RGB→index mapping.** Palette = the JS-exposed tables
  (`@opentui/core` `src/lib/RGBA.ts`, compiled at
  `dist/chunk-bun-sjw2d9bq.js:1030-1105`: `ANSI16_RGB`,
  `ANSI_256_CUBE_LEVELS = [0,95,135,175,215,255]`, gray ramp
  `8+10n`, `ansi256IndexToRgb`). The forward mapping is raster-
  internal (no ref source available): a 144-color sweep (gray ramp,
  every default-theme constant, cube corners, seeded randoms)
  rendered through the ref raster under `TERM=xterm-256color`
  (clean-env PTY) shows it is NEAREST PALETTE MEMBER BY SUM OF
  SQUARED CHANNEL DIFFERENCES, LOWEST INDEX WINNING TIES
  (`#ffffff`→15 not the equidistant cube corner 231; the exact-tie
  gray 13,13,13 → 232 over 233) — 144/144 match. NB: the ref's mini
  scrollback uses a DIFFERENT quantizer (oklab-weighted
  `nearestIndexed`, `packages/tui/src/mini/theme.ts:95-110`) — that
  is the `opencode run` path, not this TUI.

### Our implementation

* `crates/oc-tui/src/render/color_mode.rs` (new): `ColorMode`,
  `detect`/`detect_from_env` (the matrix above), `palette_rgb`,
  `quantize`, `ColorCache`, `degrade_frame` — plus unit tests: the
  capability matrix, the sweep-observed theme table, tie-breaking,
  palette round-trip, cache behavior.
* `crates/oc-tui/src/app.rs`: one-shot `detect_from_env()` at boot;
  `degrade_frame` applied to the frame right before the ratatui draw
  — the terminal-draw seam, mirroring v2's split (framebuffer RGB,
  emission indexed). `App::frame()` and every golden stay truecolor.
* `tools/parity/src/tui_frame.rs`: `nearest_ansi256` was already
  algorithmically identical to the raster; its "approximation" docs
  are upgraded to sweep-verified and a sweep-pinning test was added.
* `tools/parity/src/tui_diff.rs`: opt-in diagnostics
  `PARITY_DUMP_RAW=1` (raw PTY mirrors) and `PARITY_DUMP_STYLES=1`
  (final-grid style digests) — the instruments this investigation
  used.

### Theme audit (finding 2) — no drift exists

`crates/oc-theme/assets/v2/opencode.json` is byte-identical to the
ref's `packages/tui/src/theme/assets/v2/opencode.json`; the resolved
tokens are golden-pinned to the `@opencode/theme` TS pipeline
(`crates/oc-theme/tests/goldens/opencode.{dark,light}.json`: bg
resolves to `#0a0a0a` on both stacks). The report's `#080808` was
v2's emitted `48;5;232` canonicalized back through the xterm-256
palette — a shadow of finding 1. Closed with it.

### Corpus after (single run, same scripts/sizes/pacing)

| metric | before (per run) | after (per run) |
|---|---|---|
| steps | 298 | 298 |
| identical | 4 | 4 |
| style-differing cells (EXACT) | every mismatched cell, whole-screen (100%) | 132,327 — confined to the component regions below |
| DISTINCT differing style pairs | unbounded (encoding differed on every cell) | **11** |

Every remaining style divergence is one of 11 (rgb,style) pairs, each
mapping to a finding:

| style pair (ref→our) | cells | finding |
|---|---|---|
| idx75/234 → idx15/234 | 45 | S1#5 composer model row (ref `Build · Fake Model` in blue) |
| idx8/234 → idx234/255 (+23/4/3 variants, nord incl.) | 67 | **NEW-S1a** composer input/placeholder row: ref muted-on-raised, ours formfield gray.600-on-gray.200 — pre-existing (present in the truecolor dumps), exposed by S0 |
| idx15/232 → default/default | 3 | S4 resize: fresh vt-grid cells after shrink (teardown detail of #19/#21) |
| bg idx0 (ref dialogs) → our theme bg | 35 | **NEW-S2a** dialog backdrop: ref fills with terminal-default black, we paint theme bg — pre-existing, exposed by S0 |
| idx238/idx0, idx15/233 details | 5 | S2 dialog geometry/row family (#7-#11) |
| idx15/232 → idx8/232 | 1 | S1#4 footer slot family |

and the largest single class — transcript body fg idx15 vs our idx255
(482 cells in long-reply-scroll@80x24 alone) — is **NEW-S3a**: v2
renders assistant/user body text with the DEFAULT foreground
(#ffffff→idx15), we use `text.base` #eeeeee. Pre-existing (present in
the truecolor dumps; ±8 canonicalization could not absorb 17/255),
exposed by S0. Remaining char divergences are the known text findings
(#3 boot model, #6 placeholder pool, #12 auto-title, #13/#14 stats).
**Nord theme values now verify equal** (#24's follow-up gate): after
`/themes` + nord, global bg/fg and raised surfaces match cell-exact;
the only nord diffs are the NEW-S1a formfield row.

### Goldens

No regen needed — by design: goldens serialize frames (truecolor,
TERM-free), never the degraded emission; v2's own golden exports are
truecolor for the same reason. Verified: full oc-tui + oc-theme
suites pass byte-identical (611/611), and a `PARITY_UPDATE=1` run
rewrites zero bytes (double-regen byte-stable). Documented in
`golden_tests.rs` ("TERM independence").

### Gates

`cargo fmt --all -- --check` clean; `cargo clippy -p oc-tui -p
oc-tui-core -p parity --all-targets` adds zero warnings (the two
pre-existing `parity` bin warnings predate the lane);
`cargo nextest run -p oc-tui -p oc-tui-core -p parity` = 721 passed
/ 11 skipped; gated entry `OC_TUI_DIFF=1 … -E 'test(tui_diff)'`
PASS (35 s). Corpus rerun: `PARITY_BUN=/tmp/opencode/bun/bin/bun
tools/dev-gate.sh 'cargo run -p parity --bin tui-diff -- --runs 1'`.

### Follow-ups handed to the next waves

* NEW-S1a composer formfield row style (67 cells corpus-wide).
* NEW-S2a dialog backdrop terminal-default black.
* NEW-S3a transcript body default-white vs text.base.
* Mini (`opencode run`) scrollback quantization uses v2's OKLAB
  `nearestIndexed` (mini/theme.ts:95) — different algorithm from the
  raster's; replicate if/when the mini host is diffed.

# Addendum — corpus phase-2 (lane/corpus2, 2026-10-11)

Nine new scripts + rig extensions (isolated worktree, one commit).
All runs: `PARITY_BUN=/tmp/opencode/bun/bin/bun tools/dev-gate.sh
'cargo run -p parity --bin tui-diff -- --filter <id> --runs 1'`,
artifacts under `/tmp/opencode/tui-diff/corpus2/`.

## Rig extensions (tools/parity/src/tui_diff.rs, all justified in-code)

1. **Per-script `size` override** (`"size": "160x36"`): the sidebar's
   v2 auto breakpoint (`width − verticalTabsWidth > 120`,
   session-frame.tsx:107) is unreachable at the corpus sizes 80/120
   cols; `--sizes` stays the global knob, a malformed spec is a loud
   corpus error.
2. **`sweep` step** (hover/click coverage mandate): `motion` feeds an
   SGR move for EVERY grid cell (batch settle — per-move settling is
   unaffordable at 1920+ cells, mandate allows batching; skipped
   cells reported per side); `click` feeds move+press+release per
   listed cell (real clicks follow a move; hover-gated affordances
   need it) with a per-click settle probe and a per-side reaction
   matrix — the click-parity signal is "cells where exactly one side
   reacted". Matrix caveats (documented): reactions are counted only
   above the 2-cell caret-blink budget, and single-shot renders
   slower than the 400ms settle budget escape per-click detection
   (the step's end-state diff is the backstop — it caught #28).
3. **Request-path parity**: the shared config's model now carries a
   `body` overlay (`temperature: 0`) — v2's sanctioned wire-field
   forcing (config ModelOverlays.body merges into the provider
   request body; the model `options` key is v1-only and silently
   dropped by v2's current schema, verified against
   packages/schema/src/config/provider.ts). The stub records each
   main request's transport shape (top-level keys minus
   `messages`/`tools`, option values, model, user-agent);
   attribution is by UA (`opencode/stable/2.0.24/cli` = ref,
   `opencode/unknown/unknown/opencode` = ours).
4. Fixed a pre-existing `unused_assignments` warning in the spawn
   path (the file must stay `-D warnings`-clean for the RELEASE.md
   gate shape).

## Phase-2 baseline rows (steps aggregated, single run per size)

| script | size | steps | identical | text-only | mismatch | boot r/o | error |
|---|---|---|---|---|---|---|---|
| tabs-live-session | 80x24 | 24 | 0 | 0 | 24 | true/true | |
| tabs-live-session | 120x30 | 24 | 0 | 0 | 24 | true/true | |
| multi-session | 80x24 | 25 | 0 | 0 | 25 | true/true | |
| multi-session | 120x30 | 25 | 0 | 0 | 25 | true/true | |
| sidebar-wide | 160x36 | 16 | 0 | 0 | 16 | true/true | |
| thought-expand-structural | 80x24 | 17 | 0 | 0 | 17 | true/true | |
| thought-expand-structural | 120x30 | 17 | 0 | 0 | 17 | true/true | |
| hover-sweep | 80x24 | 17 | 0 | 0 | 17 | true/true | |
| hover-sweep | 120x30 | 17 | 0 | 0 | 17 | true/true | |
| multi-turn | 80x24 | 24 | 0 | 0 | 24 | true/true | |
| multi-turn | 120x30 | 24 | 0 | 0 | 24 | true/true | |
| click-coverage-home | 80x24 | 5 | 1 | 0 | 4 | true/true | |
| click-coverage-session | 80x24 | 15 | 1 | 0 | 14 | true/true | |
| click-coverage-dialog | 80x24 | 9 | 0 | 0 | 9 | true/true | |

- 0 driver errors anywhere; the two `identical` steps are post-exit
  blanks (`ctrl+c` exits both from home/session — same teardown
  parity as #help-dialog's).
- Hover coverage: **complete** — home, session-with-transcript and
  open-dialog sweeps fed every cell on both sides, 0 skipped
  (1920/1920 at 80x24, 3600/3600 at 120x30, per side).
- Click coverage: complete per documented class map (below), at the
  coverage size 80x24.

## New findings (classified; continuation of the backlog numbering)

26. **(a) REQUEST-PATH — model `body` overlay dropped by our stack**:
    the shared config forces `temperature: 0` via the model's `body`
    overlay; the stub records `temperature` in EVERY ref main
    request (chat AND title-generation) and in NONE of ours
    (multi-turn: ref 4/4 carry it, ours 0/5; multi-session 4/4 vs
    0/4). UA-attributed, deterministic.
27. **(a) REQUEST-PATH — request-count drift**: across 3 turns ours
    issued 5 main requests where ref issued 4 (the extra one is a
    no-`max_completion_tokens` request — the title-generation path
    fires an extra time on our side). The conditional
    `max_completion_tokens` parity itself is exact (both send it on
    primary requests only).
28. **(a) TAB-STRIP `+` HIT-ZONE OVER-WIDE (ours)**: our Add zone is
    the full 3-col `" + "` cell incl. padding
    (session_tabs.rs `strip_hit`, ADD_TAB_WIDTH=3) — clicks on the
    padding cells (no visible affordance) navigate home on OUR side
    only; ref shows no reaction there. Found by the no-op click
    class consuming the session state (end-state diff; the per-click
    matrix missed it — slow single-shot render, see caveats).
29. **(a) THOUGHT-TOGGLE HIT-ZONE (ours)**: our expand toggle reacts
    across the whole header row (row-6 OUR-only reactions across
    cols 0-79 in the session no-op matrix); ref's visible reactions
    concentrate at the glyph cells.
30. **(a) THOUGHT EXPAND GEOMETRY**: expanded reasoning renders
    inside a `┃` gutter on ref (`┃ Let me think…`), bare on ours,
    and the reply block below shifts one row differently. Toggle
    itself works on both (expand AND collapse recorded).
31. **(a) DIALOG OUTSIDE-CLICK DISMISSAL missing (ours)**: ref closes
    the /help dialog on ring/margin clicks (observed mid-sweep: Help
    text gone after the ring class); ours stays open through ALL
    1248 outside + 288 ring clicks.
32. **(a, needs 1-confirm) ctrl+c WITH OPEN DIALOG**: ref exits
    (final grid blank), ours still fully rendered at the exit-step
    capture (click-coverage-dialog last step).
33. **(a) FOOTER/COMPOSER CELL REACTIVITY (ours inert)**: ref reacts
    (hover/click styling) to footer hint cells (rows 17/22/23 spans)
    and scattered composer-adjacent cells on home and dialog
    screens; our side showed ZERO click reactions on those screens.
34. **(a) TRANSCRIPT CLICK-SELECTION (ours inert)**: ref reacts to
    transcript-row clicks (rows 2-4 of the session screen,
    selection rendering); ours inert across the whole no-op class.
35. **(a) SIDEBAR CONTENT GAP**: at 160x36 the sidebar renders on
    BOTH (getting-started card byte-visible on both — parity
    positive) but ref shows a Context section (`Context / 18 tokens
    / 0% used`) that ours lacks (title row only); footer/border seam
    rows also differ (the #20 family at a new width).
36. **RIG OBSERVATION (both stacks, NOT a divergence)**: leader
    chords `<leader>n` (session.new), `<leader>w` (tab close),
    `<leader>b` (sidebar toggle) produce NO grid change in the rig
    on EITHER stack (burst AND split pacing, with and without live
    sessions), while `<leader>l` opens the browser on ref (#16) and
    `alt+down/up` cycle on both. `<leader>q` dispatches
    `session.queued_prompts` (keybind.ts:131 SHADOWS app.exit's
    `<leader>q`) — byte-invisible on an empty queue, so the rig's
    historical `\x18q` auto-exit fallback is inert and teardown
    always relied on the kill path. Scripts that need a VISIBLE exit
    use `ctrl+c`. Root cause of the n/w/b no-ops is unexplained
    (v2's dispatch layers look correct on paper); both stacks agree,
    so the differential rows stay comparable.
37. **SCRIPT-BUG CORRECTION (existing corpus)**: `thought-expand.json`
    clicks (4,8) but the thought header sits at row 6 — the legacy
    click hits the reply text and never toggled anything on either
    side. The new script clicks (6,6) and toggles on both.

### Parity positives worth protecting (phase-2 evidence)

- Tab creation via the strip `+` affordance: opening class 25/25
  reacted-BOTH; both stacks navigate to the new-tab home state.
- `alt+down`/`alt+up` cycle: transcript bodies switch on both.
- Thought expand + collapse toggle: both, in both directions.
- Sidebar mounts at 160x36 on both (auto semantics agree).
- Request shape symmetry (minus #26/#27): identical key sets,
  identical conditional `max_completion_tokens`, identical stream/
  store flags — ref vs ours differ ONLY in `temperature` presence.
- Hover coverage: 0 skipped cells anywhere; the #18 hover lifecycle
  stays intact under whole-grid motion.

## Click class maps (documented per the mandate)

Predicted at authoring; observed refinements noted. Cells outside
the listed affordances = no-op class (clicked in full). Destructive
classes run LAST; the session screen orders nav → no-op → opening →
destructive because the `+` (opening) consumes the session view BY
DESIGN on both stacks.

- **HOME (80x24)**: selection = composer block rows 12-16 cols 2-77;
  opening/destructive = none predicted. Observed: ref-only reactions
  on footer row 17 spans + a few rows 13-15 cells (#33).
- **SESSION (80x24, reasoning reply)**: nav = strip pill (row 0
  cols 3-25) + composer rows 12-16 cols 2-77; opening = thought
  header row 6 cols 2-25 + `+` (row 0 col 33); destructive = close
  zone row 0 cols 26-30 (hover-revealed ✕); no-op = rest (observed
  Add-box cols 31-35 EXCLUDED after #28; row 6 excluded after #29).
  Observed: opening 25/25 both; destructive REF-only (our ✕ needs a
  latched hover state, 60ms motion→press is too fast to arm it);
  no-op REF-only on transcript rows (#34).
- **DIALOG (/help, 80x24)**: selection = interior rows 8-15 cols
  16-63; no-op = border/padding ring; destructive = outside margins
  (rows 0-5/18-23 full width + side bands). Observed: ref dismisses
  from the RING (#31); ours never dismisses; ref-only reactions on
  scattered interior cells.

## Request-path parity section

Every script with submits now records the stub's view of both
stacks' main requests (shape lines in the report). Baseline summary:
ref = `temperature` present + 1 title-request per session-turn
cadence; ours = `temperature` absent (#26) + extra title-path
request (#27). This section is the standing regression surface for
the config→wire pipeline on both stacks.

# Addendum — corpus3: subagent differential (lane/corpus3, 2026-10-11)

Three new scripts (`subagent-basic`, `subagent-browser`,
`subagent-sidebar`) walk a REAL subagent flow through the shared stub
on BOTH stacks, then diff the TUI surfaces: transcript tool-card
rendering, the subagent picker / browser chrome, tab titles, sidebar
suppression on the child session, and exit states. All runs:
`PARITY_BUN=/tmp/opencode/bun/bin/bun tools/dev-gate.sh 'cargo run -p
parity --bin tui-diff -- --filter subagent --runs 1'` (the filter now
matches by stem PREFIX), artifacts under `/tmp/opencode/tui-diff/`
(`report.md` + `subagent-*/` per-run dumps; probe evidence under
`/tmp/opencode/corpus3-probe/`).

## Rig extension: the attribution-keyed flow stub

`SharedStub` gained a second arming mode (`stub: "subagent-flow"`):
instead of one fixed SSE script, each MAIN request is answered by
ATTRIBUTION — body-substring matching, never request order (corpus2
#27 showed cadence drift misaligns any ordering scheme):

| request | attribution (body substring) | SSE reply |
|---|---|---|
| main | `max_completion_tokens` present, no tool result yet | one `delta.tool_calls` entry: tool `subagent` (v2 tool name, `packages/core/src/tool/plugin/subagent.ts:10`), args `{agent: "general", description: "Check the fixture", prompt: "PARITY-SUBAGENT-TASK …"}` |
| subagent | `"You are a subagent spawned by another session."` (the child's first-prompt prefix — byte-identical on both stacks: subagent.ts:209-212 / our `FIRST_PROMPT_PREFIX`) | reasoning delta + `Subagent fixture done.` |
| continuation | `max_completion_tokens` present + the subagent's reply text (the tool result) | `Main flow final reply.` |
| title | no `max_completion_tokens` (the corpus2-#27 split) | `Parity Subagent Flow` (fixed title → comparable tab titles) |

`general` is the built-in subagent-mode agent on BOTH stacks (ref
`packages/core/src/plugin/agent.ts:88-95`), so the flow never depends
on config-defined agents (see #38). Single-turn by design (the
continuation marker enters history after exchange 1); documented
in-code. `RecordedRequest` gained the attribution `kind` so
request-count drift is visible PER FLOW STAGE, and the request-path
report section renders it.

## Baseline rows (single run per size)

| script | size | steps | identical | text-only | mismatch | boot r/o | error |
|---|---|---|---|---|---|---|---|
| subagent-basic | 80x24 | 14 | 0 | 0 | 14 | true/true | |
| subagent-basic | 120x30 | 14 | 0 | 0 | 14 | true/true | |
| subagent-browser | 80x24 | 21 | 0 | 0 | 21 | true/true | |
| subagent-sidebar | 160x36 | 19 | 0 | 0 | 19 | true/true | |

0 driver errors; both stacks booted everywhere; the flow COMPLETED on
both stacks (request-path: ref = main 1 + subagent 1 + continuation 1
+ title 1; ours = main 1 + subagent **2** + continuation 1 + title 1).

## New findings (continuing the backlog numbering)

38. **(a) CONFIG `agents` MAP NOT APPLIED (ours)**: a config-defined
     agent never reaches our agent registry (`location.rs`
     `AgentRegistry` — the config lane is wired as
     `TODO(lane-integration)`); a `subagent` tool call naming one fails
     `Unknown agent: <id>`. Differential proof (manual probe,
     `/tmp/opencode/corpus3-probe/`): identical config with
     `agents.researcher` + a researcher tool call — the REF resolved it
     and completed the flow (main → subagent → continuation → title);
     ours answered every attempt with
     `{"error":{"message":"Unknown agent: researcher","type":"tool.execution"}}`.
     The ref loads config agents in
     `packages/core/src/config/plugin/agent.ts:100-127`. Rig
     consequence: the flow targets the built-in `general` agent.
39. **RIG OBSERVATION (both stacks, with an ours-side cadence note)**:
     a persistently failing tool call (deterministic stub re-issuing
     it) loops WITHOUT BOUND on BOTH stacks — probe with a nonexistent
     `ghost` agent, 25 s each: ours 613 requests, ref 59 (both unbounded;
     ours ~10x the cadence). Not registered as (a) since the ref loops
     too; the cadence difference is worth a look when the engine's
     retry pacing is revisited.
40. **(a) SUBAGENT TOOL CARD MISSING (ours)**: the ref transcript
     renders the completed tool call as an InlineTool card
     `✓ General Subagent — Check the fixture` between the user text
     and the final reply (routes/session/index.tsx:3087-3120); ours
     renders NOTHING for the subagent tool part at any capture
     (running or settled) — the card row is absent from our
     transcript entirely.
41. **(a) REQUEST-PATH — DUPLICATE SUBAGENT REQUEST (ours)**: ours
     issued exactly TWO subagent-classified primary requests (both
     carrying `max_completion_tokens`, so not the title path) where
     the ref issued one — deterministic across all 6 executions
     (3 scripts + tuning reruns). Root cause uninvestigated (out of
     lane scope; oc-core is off-limits here).
42. **(a) #26 EXTENSION — temperature overlay absent on ALL flow
     requests (ours)**: main, subagent, continuation AND title
     requests on the ref carry `temperature: 0`; ours drops it on
     every one (same root as #26, now measured across the subagent
     paths too).
43. **#16 CORRECTION (attribution)**: `leader+l` was a NO-OP on the
     REF in this corpus as well (session screen, live session, live
     subagent — no dialog ever opened). The "sessions browser
     (Subagents / Shell / Terminals tabs)" recorded in #16 was the
     COMPOSER panel opened by the following `down` keypress
     (`session.child.first` = "Toggle subagent picker", keybind.ts:136).
     #16's ours-side observation (no-op) stands; the ref-side
     attribution is corrected: neither stack opens a leader+l dialog
     in this rig.
44. **(a) SUBAGENT PICKER MISSING (ours)**: on the ref session screen,
     `down` opens the composer with tabs `Subagents  Shell  Terminals`,
     body `No active subagents`, hint `show inactive ctrl+a`;
     `ctrl+a` flips the filter and reveals the completed run
     (`General: Check the fixture`), `down` selects it, `enter`
     (composer.subagent.select) navigates into the child session.
     Ours: `down` is inert — our keymap table LISTS `session.child.first`
     (oc-tui-core keymap/table.rs:490) but oc-tui has no handler, so
     the subagent session is KEYBOARD-UNREACHABLE in our TUI.
45. **(a) CHILD-SESSION VIEW UNREACHABLE (ours; ref surfaces verified)**:
     on the ref, navigating into the subagent session shows the child
     transcript — first-prompt prefix + task, `+ Thought · 8ms` (the
     thought block renders inside the subagent run), the reply, stats
     `General · Fake Model · 93ms · 225.8 tok/s` — and at 160x36 the
     SIDEBAR IS ABSENT (session-frame.tsx:106-111, parentID sessions
     never show it — verified live, full-width transcript). Ours cannot
     reach the child view from the TUI (#44); our code-level
     suppression exists (store.rs:5120 unit test) but is
     TUI-unobservable in this rig.
46. **(a) EXIT FAREWELL SCREEN MISSING (ours)**: `ctrl+c` from a live
     session: the ref renders the opencode wordmark plus
     `Session  Parity Subagent Flow` and `Continue  opencode -s <id>`
     (a resume hint); ours exits straight to a blank grid. Visible in
     every corpus3 exit step. (The earlier corpus "identical post-exit
     blanks" were from HOME with an empty composer — from a session
     the ref prints this screen.)
47. **(a) SIDEBAR CONTENT GAP on the subagent corpus (#35 re-verified)**:
     at 160x36 the ref sidebar shows `Context / 18 tokens / 0% used`
     under the session title; ours shows the title row only. Plus the
     #12 family: the ref sidebar title reads the generated
     `Parity Subagent Flow`, ours stays `Untitled session` (the
     title REQUEST/RESPONSE itself is at parity — 1 title request on
     each side, served the same fixed string; the display gap is #12).

### Parity positives worth protecting (corpus3 evidence)

- The full flow completes on BOTH stacks: tool call executed, child
  session spawned with the byte-identical first prompt, reasoning +
  reply rendered, tool completed, continuation answered, one title
  request each — the stub's attribution held under the request-count
  drift (#41) exactly as designed (ordering-free).
- Ref-side surfaces now pinned for the fix waves: the InlineTool card
  format, the composer tab labels (`Subagents  Shell  Terminals` +
  per-state hints), the inactive-filter interaction (ctrl+a), the
  child-session transcript shape, and the sidebar suppression at the
  child route.
- Exit teardown stays clean on both stacks (no hangs; the grids
  settle and the alt-screen leave is observed).

# Addendum — fix-wave E2: the subagent UI surfaces (lane/subui, 2026-10-12)

The four class-(a) TUI findings of the corpus3 backlog, fixed and
rig-verified. All runs:
`PARITY_BUN=… OC_TUI_DIFF` gates through `tools/dev-gate.sh`;
differential `cargo run -p parity --bin tui-diff -- --filter subagent
--runs 1` (artifacts under `/tmp/opencode/tui-diff/`).

## #40 — the subagent tool card

**Root cause (ours)**: `Part::Tool` carried only `{id, name, time}` —
the wire `part.state` (`{status, input, metadata}`) was dropped at
every ingest seam, and the transcript's `Part::Tool` render arm
returned an empty row. The ref renders the card via `Subagent`
(routes/session/index.tsx:3087-3120) over `InlineTool`/
`InlineToolRow` (message-parts.tsx:198-275).

**Port**: `ToolState`/`ToolStatus` on the model
(components/message/model.rs) + the live reducers the v2 client runs
(data.ts:919-980 — `tool.input.started` streaming reset, `called`
running+parsed input, `progress` metadata swap, `success/failed`
settlement) + `stringify_value/map` (`stringValue` semantics) at both
ingest seams (typed wire + raw events) +
`rows::subagent_tool_row` (completed `✓ {Titlecase(agent)} Subagent —
{description}`; running spinner+label — the `spinner` arm renders
`children`; streaming `⠋ Delegating…`; continuation `↳ Continue
subagent — …`; the `· {model}` suffix) wired through the
`Entry::Part` arm for canonical tool name `subagent`, with the
child-session live status (`metadata.sessionID` → running map) fed
through `TranscriptOptions.subagent_running`.

**Rig**: our s09 = the ref row-for-row (`✓ General Subagent — Check
the fixture` at the ref's row/cols/muted style); only #12 (strip
title) and #13 (stats row) cells remain at those steps.

## #44 — the subagent picker

**Root cause (ours)**: the keymap table listed `session.child.first`
but no handler existed — `down` fell to `input.move.down` and the
panel was unreachable.

**Port**: `PickerState` (open/tab/filter/selection — v2's Session
`composer` store, index.tsx:217-220 + the tab's memory filter,
subagents-tab.tsx:36-37) in the store; the panel component
(components/subagent_picker.rs — tabs `Subagents  Shell  Terminals`
+ right-aligned `esc`, the 5-row body (` No active subagents` /
`{agent}: {title}` rows in the action.primary palette, focused =
peach+bold), the `show {inactive|active} ctrl+a  tabs ←/→` hints row;
box = border col 2 over raised, content [4, W−5]) mounted in place of
the whole prompt mount (`open={composer.open || parentID}`,
index.tsx:1451 — the ╹▀/status rows belong to the prompt); the
`composer` keymap MODE (table entries `composer.*` +
`session.child.next/previous` mirroring keybind.ts:136-138) pushed
while live so escape/ctrl+c close and the arrows/enter/ctrl+a drive
the list; `session.child.first` resolves ABOVE `input.move.down` on
an empty draft (stroke_matches_command — the corpus3 evidence that
`down` opens the panel on the ref); the family walk
(`sessionFamily`, util/session.ts) over the extended session registry
(agent/title/created/running off `session.created`/`renamed`/
`agent.selected`/`execution.*`, seeded by the resync snapshot).

**Rig**: our s14/s15 are byte-shaped with the ref (tabs row, `esc` at
cols 73-75, `No active subagents`, the ctrl+a flip to `General: Check
the fixture`, both hint states); enter navigates into the child (no
new tab — the strip keeps the root's pill).

## #45 — the child-session view + sidebar suppression

**Port**: navigation roots the tab bookkeeping
(`session-tabs.tsx`'s `root()` — context/session-tabs.tsx:138,188):
`Store::navigate` uses the family root, so a child route never opens
a tab and the strip's active pill stays the parent's; the child's
transcript renders from the live event trees; the child route ALWAYS
mounts the picker (the `parentID` composer) and its close NAVIGATES
to the parent (index.tsx:1453-1460 — v2's keyed remount resets the
composer state, mirrored by clearing the open flag). The sidebar
suppression itself (`session-frame.tsx:106-111`) was already ported
(store `sidebar_shown`) — this lane made it TUI-OBSERVABLE.

**Rig**: our s16/s17 (160×36): the child's transcript (user box
`You are a subagent spawned…`, thought, reply) renders with the
picker at FULL width (`esc` past col 118) and NO sidebar pane; esc
re-docks the parent's sidebar (Getting started card back) and
restores the prompt. The 80×24 child step's row shifts trace to #41
(the duplicate subagent request doubles the child transcript —
server-side lane G), not to the view.

## #46 — the exit farewell screen

**Root cause (ours)**: the loop tore down to a blank primary screen;
the ref prints the session epilogue after the renderer stops
(app.tsx:443-450 → `sessionEpilogue`, util/presentation.ts:26-34).

**Port**: `presentation.rs` — a byte-exact port of `sessionEpilogue`
(the per-glyph SGR wordmark over logo.ts, `_`/`^`/`~` cell kinds with
the 235/238 palettes, `weak()` 10-padded dim labels, bold values,
`Locale.truncate(_, 50)` UTF-16 semantics); `Store::exit_epilogue`
stages it from the routed session's registry title; the run loop
writes it + `\n` to stdout AFTER `LeaveAlternateScreen` (home exits
print nothing — the corpus's identical post-exit blanks).

**Rig**: all four executions' exit steps are now **text-only, 16
cells, styles identical** — the `Continue  opencode -s <id>` session
IDS alone differ (each stack generates its own). The wordmark + both
label rows are byte-identical through the SGR digests.

## Verification (this lane)

- Rig (before → after): 68 mismatched steps → 64 mismatched + 4
  text-only exits; the picker/child/card surfaces render byte-shaped;
  every remaining cell maps to OTHER lanes' findings (#12 auto-title,
  #13 stats row, #14 thought duration, #41 duplicate subagent request
  — server-side lane G, and the B/C/D chrome families), none to E2's
  list.
- Tests: `pty_subagent.rs` — the live-loop walk over a scripted
  subagent flow (card → down → ctrl+a → enter → child at 80 and 160
  (sidebar absent) → esc → ctrl+c farewell with the epilogue +
  home-exit negative); unit pins for the card states (streaming/
  running/completed/continuation/model-suffix), the tool-event state
  machine, the registry family/status, the picker walk (open/filter/
  navigate/close-to-parent/wrap-move), the epilogue bytes/route
  tracking, and the panel geometry (both corpus shapes).
- Gates: `cargo fmt --all -- --check` clean; `cargo clippy -p oc-tui
  -p oc-tui-core -p parity --all-targets` adds ZERO warnings (the 45
  `parity` warnings predate the lane — main-identical);
  `cargo nextest run -p oc-tui -p oc-tui-core -p parity` 745/745
  (`pty_sidebar_plugin_section_renders_through_the_host` is a
  load-dependent bun-cold-start flake — passes in isolation on main
  and here); the gated entry
  `OC_TUI_DIFF=1 … -E 'test(tui_diff)'` PASS.
- Goldens: `PARITY_UPDATE=1` regen rewrites ZERO bytes (double-regen
  byte-stable; nothing to commit) — the new surfaces render only
  under live state the fixtures never carry.
