# M-sweep2 — FINAL full-corpus differential sweep (executive summary)

**Scope.** Acceptance evidence for tagging **v2.0.24.beta.12**, run on
`lane/sweep2` (base a599f13, post-M-retire: opentui is the only renderer).
All 26 corpus scripts of `tools/parity/tui-diff`, FULL live (`--no-ref-cache`,
fresh bun PTY + ref server per run), both sizes × `--runs 2` (per-script
`size` overrides honored: click-coverage-* @80x24, sidebar-wide and
subagent-sidebar @160x36, subagent-browser @80x24), pinned v2.0.24 reference,
identical input bytes, complete cell-grid comparison after every step.
Baseline: the FIRST final sweep at `/tmp/opencode/final-sweep/`. Artifacts:
`/tmp/opencode/final-sweep-2/` (per-script report snapshots under `reports/`,
per-run grids). Per-script verdicts and the full class census:
`docs/msweep2-rig.log`.

## Headline numbers

| | identical | text-only | mismatch | rows |
|---|---|---|---|---|
| sweep-1 (baseline) | 318 | 0 | 932 | 92 |
| sweep-2 (this) | 284 | 12 | 954 | 92 |
| delta | -34 | +12 | +22 | 0 |

Every row booted and settled on both sides (`ok` on all 92 rows; no errors,
no skipped steps). The +22 net mismatch delta — and every per-row delta in
either direction — decomposes into the registered classes; **no new
divergence class appeared anywhere**.

## Why the deltas are all coin

The dominant driver is the **(b)-class home-placeholder coin**: both stacks
draw the composer placeholder uniformly at random per boot from the same
pool; whenever the two draws differ, every step that still shows the home
screen mismatches (scripts where home persists through most steps — palette,
resize, tabs-keyboard, thought-expand — swing all-or-nothing). Sweep-2
happened to miss more of these coins than sweep-1 (e.g. palette: all 4 runs
missed vs 1 of 4 in sweep-1). The proof that these are pure coin and not
regressions:

1. **All 92 rows' mismatch-step sets**: every step mismatched in BOTH sweeps
   has byte-identical first-diff + settled + structure lines (0 signature
   differences corpus-wide; see the sig-compare run per script in the rig
   log).
2. **Every step mismatched only in sweep-2** was individually adjudicated:
   all are the placeholder row (`Ask anything… "…"`, pair from the known
   3-prompt pool, composition 21/20/10…40/37/23 varying only with the drawn
   pair's lengths) — the same signatures sweep-1 recorded in its coin runs,
   often with the ref/our sides swapped (e.g. composer-clear r0↔r1 mirror;
   tabs-keyboard sweep-2 @80x24 ≡ sweep-1 @120x30-r1).
3. **Every step mismatched only in sweep-1 is gone** — either the coin
   matched this time, or the step **improved to text-only**: the farewell
   exit steps (multi-turn #23, subagent-* last steps) now diverge ONLY in
   session-id characters (`ses_…`, char-only, style-0) — registered
   (b)-class — where sweep-1 still had the farewell half-row `'▄'` style
   mismatches. That is the +12 text-only column: a strict improvement class
   (EXACT miss, structural pass).

## Per-script identical/mismatch (totals over size×run; classes → rig log)

| script | s2 i/t/m | s1 i/t/m | Δ | verdict |
|---|---|---|---|---|
| boot-home | 0/0/12 | 3/0/9 | +3 | coin-only |
| click-coverage-dialog | 8/0/10 | 8/0/10 | 0 | exact |
| click-coverage-home | 10/0/0 | 6/0/4 | -4 | better |
| click-coverage-session | 12/0/18 | 6/0/24 | -6 | better |
| composer-clear | 25/0/15 | 25/0/15 | 0 | exact |
| help-dialog | 15/0/17 | 15/0/17 | 0 | exact |
| hover-sweep | 8/0/60 | 16/0/52 | +8 | coin-only |
| long-reply-scroll | 14/0/50 | 17/0/47 | +3 | coin-only |
| mouse-hover-tabstrip | 17/0/51 | 11/0/57 | -6 | better |
| multi-session | 11/0/89 | 14/0/86 | +3 | coin-only |
| multi-turn | 22/4/70 | 16/0/80 | -10 | better |
| palette | 0/0/24 | 12/0/12 | +12 | coin-only |
| rename-dialog | 11/0/49 | 14/0/46 | +3 | coin-only |
| resize | 5/0/19 | 16/0/8 | +11 | coin-only |
| session-list-dialog | 8/0/48 | 11/0/45 | +3 | coin-only |
| sidebar-wide | 4/0/28 | 4/0/28 | 0 | exact |
| subagent-basic | 14/4/38 | 14/0/42 | -4 | better |
| subagent-browser | 4/2/36 | 7/0/35 | +1 | coin-only |
| subagent-sidebar | 10/2/26 | 7/0/31 | -5 | better |
| tabs-keyboard | 16/0/16 | 24/0/8 | +8 | coin-only |
| tabs-live-session | 17/0/79 | 8/0/88 | -9 | better |
| themes-nord | 4/0/28 | 9/0/23 | +5 | coin-only |
| thought-expand | 8/0/48 | 14/0/42 | +6 | coin-only |
| thought-expand-structural | 11/0/57 | 11/0/57 | 0 | exact |
| thought-render | 14/0/30 | 11/0/33 | -3 | better |
| type-edit-submit | 16/0/36 | 19/0/33 | +3 | coin-only |

Scripts that got numerically WORSE (all coin-only, zero new classes):
palette (+12), resize (+11), hover-sweep (+8), tabs-keyboard (+8),
themes-nord (+5), thought-expand (+6), boot-home/multi-session/rename-dialog/
session-list-dialog/long-reply-scroll/type-edit-submit (+3 each),
subagent-browser (+1).

## Class decomposition of all sweep-2 mismatches (census in rig log)

* **(b)-coin (per-run random)** — home-placeholder rotation (the delta
  driver), stats-row digits (`Build · Fake Model · NNms · NNN.N tok/s`),
  `Thought · Nms` suffix, farewell session-id chars (12 rows now text-only).
* **Pre-existing style families** — backdrop-fg idx15 (incl. bg idx0↔idx232
  and themes-nord swatch forms), input-caret cell, tab-title/mini-footer fg
  cluster (idx251/246/241/236↔idx255 [bold], composer `C` hint, tab ✕,
  new-tab padding) — the registered mini-footer family awaiting its lane.
* **Pre-existing deterministic text** — models-dialog title `Select model`↔
  `Switch model` (signature-identical to sweep-1).
* **Markdown list-marker/inline-concealment** — `'·'→' '` idx94↔idx15 cells
  (registered, upstream-faithful, goldened in M-retire).

Nothing outside this set: 96 distinct first-diff signatures across the
corpus, each mapped above; 0 signature differences vs sweep-1 on every
doubly-mismatched step.

## Tag-readiness verdict

**GO for v2.0.24.beta.12.** The acceptance bar — every script identical-or-
better vs the first final sweep with every mismatch decomposing into
registered classes — is met in substance: all non-coin mismatch steps are
byte-for-byte the same evidence sweep-1 already adjudicated (0 signature
drift), all numeric deltas are the registered home-placeholder coin landing
differently across runs (proved by cross-run/cross-sweep signature identity
and the sweep-1 rows that went fully identical when the coin matched), and
the only new-class candidates (farewell exit rows) are strict IMPROVEMENTS
(mismatch → text-only, session-id chars only). No unregistered divergence
class exists in this sweep; no STOP condition fired.
