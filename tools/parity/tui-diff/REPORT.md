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
