# tui-dump — reference golden-frame exporter (wave 7.0)

Drives the **reference v2.0.24 TUI's opentui renderer headless** through its
own test fixture harness (`testRender` from `@opentui/solid`, fixtures from
`packages/tui/test/fixture/*`) and exports rendered frames as goldens for the
Rust port's Phase 7 visual-parity acceptance.

This is a mirror of the working copy at `/tmp/opencode/tui-dump/` (the
runtime copy, next to the bun reference checkout at `/tmp/opencode/ref`,
self-heals its `node_modules` symlinks via `link_modules.ts`).

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
  byte-identical (asserted in-process); `--verify` re-renders everything and
  compares SHA-256 against the on-disk manifest (asserted across runs).

## Output

`goldens/tui-frames/<scenario>.w<width>.{txt,ansi}` + `manifest.json`
(camelCase, byte-stable, SHA-256 pinned). 18 scenarios × 2 widths
(80/160) × 2 formats = 72 blobs. Format details: `frame.ts` here and
`tools/parity/src/tui_frame.rs` (parser/serializer + comparators);
`crates/oc-tui/src/spike` renders the same format from ratatui.

## Scenario inventory

| id | source test | class |
|---|---|---|
| tool-wrap-rows, tool-wrap-error-expanded | cli/tui/inline-tool-wrap-snapshot | tool-row |
| session-tabs-horizontal, session-tabs-vertical | component/session-tabs-status | session-tabs |
| footer-prompt-idle, footer-permission, footer-form | mini/footer.view | mini-footer |
| permission-reject-field | mini/footer.view | mini-footer-permission |
| footer-command-menu, -model-select, -agent-select, -variant-select, -settings | mini/footer.view | mini-footer-panel |
| form-authorization | cli/tui/form | prompt-composer |
| command-palette | cli/tui/command-palette | dialog |
| dialog-select | cli/tui/dialog-select | dialog |
| group-view-instructions | cli/tui/group-view | session-group |
| diff-viewer-split | cli/tui/diff-viewer | diff-viewer |

Animations are disabled and spinner/time-driven content deliberately
excluded so frames are byte-stable.
