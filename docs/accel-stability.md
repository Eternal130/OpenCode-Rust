# lane/accel — ref-capture stability measurement (the cacheability table)

The mandate: *measure first on 3 representative scripts which steps
are capture-stable across 5 reruns; implement caching ONLY for the
stable steps; report the coverage percentage.*

Method: `tui-diff --measure-stability 5 --sizes 80x24 --filter <s>`
— 5 independent LIVE runs (cache disabled, fresh servers + homes +
project per run), per-step ref-grid digests (char + style) compared
across the runs. A step is **stable** iff all 5 digests are identical
— the bar the ref-capture cache requires (a step whose ref grid flips
run-to-run would change cached verdicts vs live ones).

## Two self-inflicted variances were found and FIXED first

1. **The footer-embedded cwd was per-run unique.** Both TUIs render
   the project path in the home footer (`/…/…/boot-home-80x24-r0/
   project:master`) and the rig's sandbox lived under the run-unique
   artifact tag → EVERY ref grid differed from the previous run's
   (first measurement: 5/5 distinct at every step, the only differing
   row the footer). Fix: the sandbox project + homes moved to a FIXED
   work root (`OC_TUI_DIFF_WORK_ROOT`, default
   `/tmp/opencode/tui-diff-work/<script>-<WxH>`), wiped per run — the
   displayed path is now identical across runs, invocations, and
   capture↔replay.
2. (Related) the artifact dir (`--out/<script>-<WxH>-r<run>`) no
   longer hosts the sandbox; it is created explicitly for the per-step
   dumps.

## The remaining variances are INHERENT ref nondeterminism (kept live)

* **The per-boot home placeholder.** v2 picks the composer placeholder
  at RANDOM per boot (`prompt/index.tsx:364 randomIndex(list().length)`
  — `Ask anything… "Fix a TODO in the codebase"` etc., pool of 3 at
  80x24). Visible whenever the home composer is empty → steps
  `boot`/`dismiss`/`variant-dismiss` show 3 distinct grids across 5
  runs. (Our port replicates the randomness — app.rs:2271 — which is
  the documented "(b)-class home-placeholder coin" of the M-lane rig
  logs.) Pinning the placeholder via config would unlock full-script
  caching but removes placeholder-pool parity from the rig's surface —
  a deliberate-surface change this lane does NOT make silently.
* **The reply stats row.** The ref renders real wall-clock duration
  and the derived rate in the transcript stats row (`Build · Fake
  Model · 156ms · 170.7 tok/s` — different every run, 5/5 distinct).
  Affects `submit`/`settled`-class steps and scrolled states where the
  stats row is on-screen (`home`/`end`/`exit` in long-reply-scroll).

## Tables (80x24, N=5)

### boot-home — 0/3 steps stable → runs fully LIVE

| step | label | distinct ref grids | stable |
|---|---|---|---|
| 0 | boot | 3 | no (placeholder) |
| 1 | dismiss-first-run | 3 | no (placeholder) |
| 2 | exit | 3 | no (placeholder) |

### type-edit-submit — 7/13 steps stable → hybrid replay

| step | label | distinct ref grids | stable |
|---|---|---|---|
| 0 | boot | 3 | no (placeholder) |
| 1 | dismiss | 3 | no (placeholder) |
| 2 | models-cmd | 1 | YES (dialog covers placeholder) |
| 3 | models-open | 1 | YES |
| 4 | models-filter | 1 | YES |
| 5 | models-select | 1 | YES |
| 6 | variant-dismiss | 3 | no (placeholder) |
| 7 | type | 1 | YES (composer text hides placeholder) |
| 8 | edit | 1 | YES |
| 9 | retype | 1 | YES |
| 10 | submit | 5 | no (stats row: ms + tok/s) |
| 11 | settled-reply | 5 | no (stats row) |
| 12 | exit | 5 | no (stats row) |

### long-reply-scroll — 8/16 steps stable → hybrid replay

| step | label | distinct ref grids | stable |
|---|---|---|---|
| 0 | boot | 3 | no (placeholder) |
| 1 | dismiss | 3 | no (placeholder) |
| 2 | models-cmd | 1 | YES |
| 3 | models-open | 1 | YES |
| 4 | models-filter | 1 | YES |
| 5 | models-select | 1 | YES |
| 6 | variant-dismiss | 3 | no (placeholder) |
| 7 | type | 1 | YES |
| 8 | submit | 5 | no (stats row) |
| 9 | settled | 5 | no (stats row) |
| 10 | pageup | 1 | YES (stats row scrolled off) |
| 11 | pageup-again | 1 | YES |
| 12 | pagedown | 1 | YES |
| 13 | home | 4 | no (stats row back on screen) |
| 14 | end | 5 | no (stats row) |
| 15 | exit | 5 | no (stats row) |

## Coverage summary

* Fully cacheable scripts (every step stable): **0/3** — every corpus
  script starts at the home screen, and the ref's placeholder pick is
  per-boot random.
* Cacheable steps across the trio: **15/29 ≈ 52%** — the models-dialog
  and typing/scroll states, exactly where the placeholder is hidden
  and no timing row is on screen.
* The settle policy is quiescence-based (`wait_grid_stable`: two grids
  250 ms apart ≤2 diff cells) with a 5 s timeout fallback — so the
  cache additionally requires the capture-time **double-capture bar**
  (two consecutive captures 250 ms apart byte-IDENTICAL, not ≤2-cell
  tolerant), and — decisively — the **multi-run confirmation** above:
  the first A/B attempt caught the warm run's within-run bar passing
  for boot-home (placeholder fixed within a run) while this
  measurement proves the flip across runs. Confirmation is now a
  hard gate: unconfirmed entries never replay.

## The confirmation lesson (why the two-tier bar exists)

The first A/B attempt warmed the cache with a single capture run and
boot-home — measured 0/3 stable — came back "all 3 steps stable": the
placeholder is chosen ONCE per boot, so two captures 250 ms apart are
always equal within a run. The double-capture bar is NECESSARY
(spinner/blink) but NOT SUFFICIENT (per-boot randomness). The cache
now stores observations UNCONFIRMED and only `--measure-stability N`
(the N independent live runs whose digest agreement IS the
cacheability table) promotes an entry to replayable.

# A/B verification record (the acceptance numbers)

Protocol: per script — warm+confirm (5 live runs × both sizes), then
(a) `--quick` timing before (`--sizes 80x24 --runs 1 --no-ref-cache`)
vs after (`--quick`, confirmed cache), (b) full-matrix timing
(`--runs 2`, both sizes) before (`--no-ref-cache`) vs after, and 5
paired verdict comparisons (uncached vs cached, 80x24).

## Speedups

| script | cacheable steps | quick before → after | full-matrix before → after |
|---|---|---|---|
| boot-home | 0/3 | 34.7s → 34.7s (±0%) | 140.5s → 140.5s (±0%) |
| type-edit-submit | 7/13 | 60.5s → 52.8s (**−13%**) | 237.7s → 212.8s (**−10%**) |
| long-reply-scroll | 8/16 | 71.1s → 65.2s (**−8%**) | 284.3s → 256.7s (**−10%**) |

* The cache's contribution is bounded by the ref's own
  nondeterminism: the hybrid replays the settle-probe + capture work
  on confirmed steps only (the ref PTY must still boot for the live
  steps). Scripts with more confirmed-stable steps benefit more.
* `--quick` itself is the bigger iteration lever: one execution
  instead of the full matrix's four — e.g. type-edit-submit iterates
  at ~53s vs ~213–238s for the full matrix (**~4.5×**), before any
  cache effect.
* Warm cost (once per fingerprint — the fingerprint covers only
  ref-side identity + script + stub, invariant across our flag
  changes): 5 live runs per (script, size): 178s / 316s / 376s for
  the trio per size pair. Amortizes after ~5 quick iterations.

## Verdict preservation (the cache must never change verdicts)

* **On replayed steps: 15/15 pair comparisons IDENTICAL** (3 scripts
  × 5 pairs; mismatch classifications agree on every step the cache
  actually supplies), and both full-matrix comparisons identical on
  replayed steps.
* Full-union differences appear in some pairs (boot-home 1/5,
  type-edit-submit 2/5, long-reply-scroll 4/5) — and a mechanical
  per-pair check proves **every differing step is a NEVER-replayed
  step** (unstable: the placeholder / stats-row coins), for all 15
  pairs.
* Control: the 7 uncached invocations per script flip among
  themselves the same way (e.g. boot-home alternates 0-mismatch and
  3-mismatch unions live-vs-live) — the documented "(b)-class
  home-placeholder coin" of the M-lane logs, pre-existing and
  untouched by the cache.

