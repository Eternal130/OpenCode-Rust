# tui-dump — reference golden-frame exporter (wave 7.0; gate-8 P1 + findings 2/5 in wave 7.1; second export wave 7.2)

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
After the 7.2 second export wave: 37 scenarios / 66 variants / 132 blobs +
3 width sidecars + 3 replay scripts (the 42 pre-existing variants are
byte-identical to the 7.1 corpus — the wave is purely additive). Format
details: `frame.ts` here and `tools/parity/src/tui_frame.rs`
(parser/serializer + comparators); `crates/oc-tui` renders the same format
from ratatui.

The manifest also pins, per gate-8:

- `theme` — the corpus theme above;
- per-scenario `class` — acceptance tier (`tier-a` / `tier-b` / `tier-c1` /
  `width-edge`, oracle-gate8 approval table);
- `comparisonDepthDefault` + per-scenario `comparisonDepth` — style
  comparison depth (`truecolor-canonical` for tier-a/c1/width-edge,
  `degraded-256` for tier-b; see `tools/parity/NORMALIZATION.md` §10);
- per-scenario `scrollOffset` (vscroll family only) — the pinned scroll
  offset of the frame (gate-9 addition a);
- `replay[]` — keypress-replay script entries (see below).

## Second export wave (gate-8 finding 2 / gate-9 additions)

- **Composer states** (`scenarios/composer.tsx`): the real session-route
  `Prompt` inside the app's own provider stack; placeholder-empty,
  typed-multiline, CJK-input, attachment-chip (`[Image 1]` file extmark)
  and autocomplete-open (`/re` filtered slash list + selected row) at 80
  and 40. States are driven through `PromptRef.set()` (the reference's own
  draft-restore path) so they are deterministic; only the autocomplete
  state uses typed keys (after `reset()` — the module-level draft stash
  would otherwise leak the previous mount's draft).
- **Message groups** (`scenarios/session_app.tsx`): full-app session route
  with consecutive user/assistant turns, exploration (`Explored: 1 read`)
  and reasoning (`Thought`) group headers, and the boundary row between
  turns, at 80 and 40.
- **Markdown frame (C1)** (`scenarios/markdown_frame.tsx`): the TextPart
  `<markdown>` mount (headings/bold/inline-code/lists/fence) with a
  ≥10-char bold run crossing the wrap boundary at BOTH widths — the
  gate-9 styled-span instrument.
- **Frozen spinner (C1)** (`scenarios/spinner_frozen.tsx`): the running
  exploration `InlineToolRow` on a `ManualClock` pinned at t=0 (spinner
  glyph ⠋, one tick, no animation loop).
- **Toast (C1)**: one toast surface over live session content via the
  app's `tui.toast.show` event channel.
- **Sidebar pair**: the right-hand session pane visible (`sidebar=auto`)
  vs hidden (`sidebar=hide`) at width 160 (the pane is `auto` only above
  120 columns).
- **Virtual-scroll pinned frames**: one transcript (10 turns ≫ a 24-row
  viewport) exported at four offsets — `vscroll-top` / `vscroll-middle` /
  `vscroll-bottom` / `vscroll-stick-bottom`; the last stays sticky while a
  new assistant turn streams in and the viewport follows the append. The
  manifest records `scrollOffset` per scenario.
- **40-column session screen**: the complete session route (transcript +
  composer + horizontal tabs bar) at width 40.
- **Keypress-replay scripts** (`replay_scripts.ts` →
  `goldens/tui-frames/replay/*.json`): non-frame acceptance fixtures for
  the future Rust key-replay harness — `leader-q` exit (binding gated on
  the empty prompt), `ctrl+p` palette open → filter → select, and
  permission once/always/deny. Every step was replayed against the booted
  reference app before being pinned; format
  `{ steps: [{ keys: [...], expect: { frameIncludes, dialogs, exit, ... } }] }`
  in kitty token encoding.

Full-app scenarios boot the entire reference app headless exactly like its
own lifecycle tests (`createTestRenderer` + `run()` against a `Bun.serve`
fixture) — deterministic data, animations off, fixed `time.*` values for
all duration labels.

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
| composer-placeholder-empty, -typed-multiline, -cjk-input, -attachment-chip, -autocomplete-open | component/prompt state corpus | composer (tier-a) |
| markdown-frame | routes/session TextPart mount | markdown (tier-c1) |
| spinner-frozen | group-view running row, ManualClock | spinner (tier-c1) |
| message-groups | full-app session fixture | message-group (tier-a) |
| vscroll-top, -middle, -bottom, -stick-bottom | full-app session fixture | vscroll (tier-a, scrollOffset pinned) |
| sidebar-expanded, sidebar-collapsed | full-app session fixture | sidebar (tier-a) |
| session-screen-40 | full-app session fixture | session-screen (tier-a) |
| toast-over-session | full-app session fixture (tui.toast.show) | toast (tier-c1) |
| replay/exit-leader-q, palette-ctrl-p, permission-once-always-deny | app-lifecycle / command-palette / input-scope | replay (non-frame) |

Animations are disabled and spinner/time-driven content deliberately
excluded so frames are byte-stable — the two C1 exceptions being the
markdown frame (async block fill awaited before capture) and the frozen
spinner (ManualClock, t=0).

