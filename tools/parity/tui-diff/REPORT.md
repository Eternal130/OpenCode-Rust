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

1. **(a) SGR color encoding**: under `TERM=xterm-256color` v2 emits
   256-INDEX SGR for the default theme (blank cells
   `fg=idx15 bg=idx232`); we emit TRUECOLOR RGB
   (`fg=#FFFFFF bg=#0A0A0A`) regardless of TERM. We do not degrade
   to the terminal's declared color capability.
2. **(a) theme value drift**: canonicalized, ref bg = idx232 →
   `#080808`, ours `#0A0A0A` (2/255/channel); fg white matches
   exactly. Same theme, different constants.

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
   tolerance. Without it the emitter-encoding difference (#1, every
   cell) makes the text/geometry classification unreachable. EXACT
   mode keeps colors byte-strict.
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
