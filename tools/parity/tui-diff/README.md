# TUI side-by-side differential rig (lane/diff-rt)

The user-mandated parity standard, mechanized: run v2.0.24's TUI and
our Rust TUI **in the same project directory**, apply **identical
operation scripts**, and compare the **complete cell grids after every
step**. This is the differential that produced the mismatch backlog in
`REPORT.md`.

## Layout

```text
tools/parity/src/tui_diff.rs      the rig (PTY pairs, stub, diffs, report)
tools/parity/src/bin/tui-diff.rs  standalone runner (prints the report)
tools/parity/tests/tui_diff.rs    OC_TUI_DIFF=1 gated test (rig-execution
                                  assertions only; content mismatches are
                                  the report's subject, not test failures)
tools/parity/tui-diff/scripts/    the JSON operation corpus
tools/parity/tui-diff/REPORT.md   the latest full-corpus mismatch report
```

## How it runs

* **Reference runtime**: the v2.0.24 checkout at `/tmp/opencode/ref`,
  self-healed from `/opt/opencode-ref` by `parity::reference`
  (tar copy + `bun install --frozen-lockfile`). The v2 TUI is spawned
  on a pty as `bun --preload <ref>/packages/cli/node_modules/@opentui/
  solid/scripts/preload.js --define OPENCODE_VERSION="2.0.24" … <ref>/
  packages/cli/src/index.ts --server <ref-server>` with **cwd = the
  shared project**. (The p10-style `bun run --cwd packages/cli` is
  deliberately NOT used: it reparents the process cwd to the checkout,
  breaking the same-project invariant; the checkout's solid-js JSX
  settings are restored by a `tsconfig.json` seeded into the project
  and the preload is passed as an absolute `--preload` flag.)
* **Both TUIs**: own HOME/XDG roots (they cannot share DBs), identical
  env otherwise; both connect `--server` to their stack's server; both
  servers read the ONE `opencode.json` in the shared project.
* **One shared SSE stub** serves both stacks
  (`parityfake/fake-model`, openai-compatible wire), so replies are
  byte-identical. Reasoning deltas (`delta.reasoning`) are understood
  by both engines, so thought blocks render on both sides.
* **Fresh servers + homes per (script, size, run)**: restored
  tabs/sessions must never leak between scripts — determinism first.

## Comparison modes

* **EXACT** — cell-by-cell char + style digest (fg/bg RGB|idx|default +
  bold/italic/underline/inverse), union grid, NO normalization. The bar
  is complete identity.
* **STRUCTURAL** — every EXACT mismatch is re-examined under two
  documented normalizations: (1) an alphanumeric glyph mask
  (`a-z0-9…` → `x`; punctuation, box-drawing, whitespace stay exact)
  and (2) color canonicalization — indexed colors resolve through the
  xterm-256 palette to RGB (v2 emits 256-indices, we emit truecolor)
  with a ±8/channel perceptual tolerance. Modifier bits and geometry
  are never relaxed. Per-row run digests compare text extents, margins
  and vertical gaps; a step whose only divergences pass this filter is
  **text-only** (model text drift with identical geometry). Chrome can
  never be masked into equality: geometry or attribute drift stays a
  MISMATCH in both modes; color identity stays required by EXACT mode
  byte-for-byte.

## Corpus

`boot-home`, `type-edit-submit`, `tabs-keyboard`, `help-dialog`,
`palette`, `themes-nord`, `session-list-dialog`, `mouse-hover-tabstrip`,
`thought-render`, `thought-expand`, `long-reply-scroll`, `rename-dialog`,
`composer-clear`, `resize` — each run at 80x24 and 120x30.

Phase-2 additions (see the addendum in `REPORT.md`):

* `tabs-live-session` — REPORT #25: tab ops with live sessions
  (creation via the strip `+` affordance — v2 has NO keyboard
  binding for tab creation — plus the alt+down/up/leader chords).
* `multi-session` — two live sessions in two tabs, leader+l browser,
  ctrl+r rename.
* `sidebar-wide` — `"size": "160x36"` per-script override; the
  sidebar auto breakpoint is unreachable at the default sizes.
* `thought-expand-structural` — expand AND collapse with captures
  (also fixes the legacy click row: the header is row 6, not 8).
* `hover-sweep` — SGR motion across EVERY cell in three states
  (home / session with transcript / open dialog), batch settle,
  skipped cells reported.
* `multi-turn` — three submit→reply rounds with editing between
  turns; transcript growth compared per round.
* `click-coverage-home` / `click-coverage-session` /
  `click-coverage-dialog` — every cell of each documented effect
  class clicked (move+press+release, per-click settle, per-side
  reaction matrix); destructive classes run LAST. Class maps live
  in the REPORT addendum.
* `subagent-basic` / `subagent-browser` / `subagent-sidebar`
  (corpus3) — the attribution-keyed SUBAGENT FLOW: the stub answers
  the main request with a `subagent` TOOL CALL, the child session's
  own request (first-prompt-prefix marker) with reasoning + a fixed
  reply, the continuation (tool result) with the final text, and
  title-path requests with a fixed title. Surfaces: the transcript
  tool card, the ref's composer subagent picker (`down` opens it,
  `ctrl+a` shows inactive runs, `enter` navigates into the child),
  sidebar suppression on the child session at 160x36, exit states.
  Findings + probe evidence: the corpus3 addendum in `REPORT.md`.

Script schema: `id`, `description`, `stub` (`text`|`reasoning`|
`reasoning-long`|`subagent-flow`), `mode` (`exact`|`struct`, headline
only — both modes are always computed), optional `size` (`WxH`,
replaces the corpus sizes for this script), `steps`. A step sets
exactly one of `keys` / `text` / `mouse` / `resize` / `sweep` (or
nothing, for a bare settle+capture) plus `wait_ms`. `sweep` is
`{action: motion|click, cells: [[x,y]…] (click), gap_ms (motion),
settle_ms (click)}`. `--filter` matches a script by exact stem OR
stem prefix (`--filter subagent` runs all three `subagent-*`
scripts).

## Request-path parity

The shared `opencode.json`'s model carries a `body` overlay
(`temperature: 0` — v2's sanctioned wire-field forcing; the `options`
key is v1-only and dropped by v2's current schema). The stub records
every main request's transport shape (top-level keys minus
`messages`/`tools`, option values, model, user-agent) and the report
renders a **Request-path parity** section per run; attribution is by
user-agent (`opencode/stable/2.0.24/cli` = ref). Baseline findings
(#26/#27 in the REPORT addendum): ours drops the overlay and issues
an extra title-path request.

Corpus3 adds the **attribution-keyed flow stub** (`stub:
"subagent-flow"`): requests are classified by BODY SUBSTRING (the
child's first-prompt prefix, `max_completion_tokens` presence, the
subagent reply text in the tool result) — never request order — and
each kind gets its own SSE payload; the recorded `kind` column makes
per-flow-stage request-count drift visible. See the corpus3 addendum
in `REPORT.md` for the design and the #38-#47 findings.

## Running

The gate user cannot read bun under `/root` (0700), and
`/usr/local/bin` is shared, mutable state — pin a stable, world-
readable bun for the rig and export `PARITY_BUN`:

```bash
mkdir -p /tmp/opencode/bun/bin
cp /root/.bun/bin/bun /tmp/opencode/bun/bin/bun && chmod 755 /tmp/opencode/bun/bin/bun

# full corpus, twice (deterministic-vs-timing split), gated:
PARITY_BUN=/tmp/opencode/bun/bin/bun tools/dev-gate.sh \
  'cargo run -p parity --bin tui-diff -- --runs 2'

# one script, one size:
PARITY_BUN=/tmp/opencode/bun/bin/bun tools/dev-gate.sh \
  'cargo run -p parity --bin tui-diff -- --filter palette --sizes 120x30'

# gated test (boot-home only; OC_TUI_DIFF_FULL=1 for everything):
PARITY_BUN=/tmp/opencode/bun/bin/bun OC_TUI_DIFF=1 cargo nextest run \
  --run-ignored only -E 'test(tui_diff)' --no-timeout
```

Artifacts land in `/tmp/opencode/tui-diff/<tag>/`: `report.md`,
per-step `sNN.{ref,our}.txt` grid dumps for every non-identical step,
`*-boot-failure.txt` raw captures.

## Normalizations (complete list)

EXACT: none. STRUCTURAL: two — the glyph mask above, and color
canonicalization (idx → xterm-256 RGB + ±8/channel tolerance; the
emitter-encoding difference v2-256-index vs our-truecolor would
otherwise mask every text/geometry finding). Cursor position is never
compared (grids carry cell contents only). Anything else must be
justified in `REPORT.md` before it is added.
