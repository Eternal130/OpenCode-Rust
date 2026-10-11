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
