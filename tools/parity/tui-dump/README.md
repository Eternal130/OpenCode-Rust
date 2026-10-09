# tui-dump — reference golden-frame exporter (wave 7.0; gate-8 P1 + findings 2/5 in wave 7.1)

Drives the **reference v2.0.24 TUI's opentui renderer headless** through its
own test fixture harness (`testRender` from `@opentui/solid`, fixtures from
`packages/tui/test/fixture/*`) and exports rendered frames as goldens for the
Rust port's Phase 7 visual-parity acceptance.

This is a mirror of the working copy at `/tmp/opencode/tui-dump/` (the
runtime copy, next to the bun reference checkout at `/tmp/opencode/ref`,
self-heals its `node_modules` symlinks via `link_modules.ts`).

## Corpus theme (gate-8 hard precondition P1)

Every scenario renders under the reference's **default theme**: the built-in
`opencode` v2 theme (`src/theme/assets/v2/opencode.json`) resolved in **dark**
mode — mounted through `ThemeProvider mode="dark" source={emptyThemeSource}`
(resolves `allThemes().opencode`), or, for the mini/ run views, through
`RUN_THEME_FALLBACK` (the same document resolved headless). The manifest
records it as `theme: { id: "opencode", label: "v2-dark", mode: "dark" }`.
The pre-P1 corpus (mixed style bases) is kept under
`goldens/tui-frames/legacy-mixed-theme/`, manifest `superseded: true` —
diffing reference only.

## Run

```bash
cd /tmp/opencode/tui-dump   # or restore this mirror there
bun --preload @opentui/solid/preload export.ts \
    --out /opt/opencode-rust/tools/parity/goldens/tui-frames
bun --preload @opentui/solid/preload export.ts --verify \
    --out /opt/opencode-rust/tools/parity/goldens/tui-frames   # determinism gate
```

- `--preload @opentui/solid/preload` is required (matches the reference
  `packages/tui/bunfig.toml`).
- `--only id1,id2` renders a subset; `--json` prints a machine summary.
- Every variant is mounted twice per run and the two captures must be
  byte-identical (asserted in-process); `--verify` re-renders everything,
  re-measures the width sidecars, and compares SHA-256 against the on-disk
  manifest (asserted across runs).

## Output

`goldens/tui-frames/<scenario>.w<width>.{txt,ansi}` + `manifest.json`
(format `tui-frame-golden/2`, camelCase, byte-stable, SHA-256 pinned).
21 scenarios × 2 widths (80/160 for the app corpus, 80/40 for the
width-edge probes) × 2 formats = 84 blobs + 3 width sidecars. Format
details: `frame.ts` here and `tools/parity/src/tui_frame.rs`
(parser/serializer + comparators); `crates/oc-tui` renders the same format
from ratatui.

The manifest also pins, per gate-8:

- `theme` — the corpus theme above;
- per-scenario `class` — acceptance tier (`tier-a` / `tier-b` / `tier-c1` /
  `width-edge`, oracle-gate8 approval table);
- `comparisonDepthDefault` + per-scenario `comparisonDepth` — style
  comparison depth (`truecolor-canonical` for tier-a/c1/width-edge,
  `degraded-256` for tier-b; see `tools/parity/NORMALIZATION.md` §10).

## Width-edge probes (gate-8 finding 2)

`scenarios/width_edge.tsx` renders display-width boundary samples —
(a) CJK text (mixed zh + ASCII, full-width punctuation, wrap-anywhere),
(b) emoji + ZWJ sequences + skin-tone modifiers, (c) combining marks /
ambiguous-width characters — each string in a wrapping box at terminal
widths 80 and 40. The same strings are measured with the `string-width`
build bundled by `@opentui/core` (the renderer's "unicode" width method:
ambiguous narrow, emoji 2 cells, combining 0) and pinned as
`<id>.widths.json` sidecars so the Rust `display_width` is unit-tested
against ground truth directly, not only via frames.

## Scenario inventory

| id | source test | class |
|---|---|---|
| tool-wrap-rows, tool-wrap-error-expanded | cli/tui/inline-tool-wrap-snapshot | tool-row (tier-a) |
| session-tabs-horizontal, session-tabs-vertical | component/session-tabs-status | session-tabs (tier-a) |
| footer-prompt-idle, footer-permission, footer-form | mini/footer.view | mini-footer (tier-a) |
| permission-reject-field | mini/footer.view | mini-footer-permission (tier-a) |
| footer-command-menu, -model-select, -agent-select, -variant-select, -settings | mini/footer.view | mini-footer-panel (tier-a) |
| form-authorization | cli/tui/form | prompt-composer (tier-a) |
| command-palette | cli/tui/command-palette | dialog (tier-a) |
| dialog-select | cli/tui/dialog-select | dialog (tier-a) |
| group-view-instructions | cli/tui/group-view | session-group (tier-a) |
| diff-viewer-split | cli/tui/diff-viewer | diff-viewer (tier-b, degraded-256) |
| width-edge-cjk, width-edge-emoji, width-edge-combining | tui-dump corpus | width-probe (width-edge) |

Animations are disabled and spinner/time-driven content deliberately
excluded so frames are byte-stable.

