# Task 2 report — ref bun driver (channel 8: TUI full-screen diff)

**Status: COMPLETE** — Steps 1–5 all pass.

## What was built

`tools/diff/drivers/tui_fullscreen_ref.ts` — headless reference driver that
boots the v2.0.24 TUI via its own `createAppFixture`
(`$DIFF_REF_ROOT/packages/tui/test/fixture/app.ts`, direct import — the
~50-line re-assembly fallback was NOT needed; declared in the file header),
feeds `tui-diff-scenario/1` fixtures (fetch stubs → `events.emit` →
`mockInput.pressKey`), waits `waitForVisualIdle({quietFrames:3,maxFrames:600})`
+ `settle_ms`, captures `captureSpans()`, and rasterizes to dense
`tui-diff-grid/1` JSON per case of a `tui-diff-manifest/1` manifest.

### Staging mapping (Step 1)

Mirrors `tools/parity/tui-dump/` with the deltas documented in the driver
header: `DIFF_REF_ROOT` (default `/opt/opencode-ref`, read-only) instead of
`OPENTUI_REF_ROOT`; no local `node_modules` symlink farm is needed because the
driver imports only ref-tree files by absolute path (their own bare imports
resolve inside the ref tree); the `@opentui/solid/preload` transform is passed
as an absolute file path (`…/@opentui/solid/scripts/preload.js`) so it works
from any cwd; environment bun 1.3.10 per controller ruling. Dev invocation
from repo root:

```
DIFF_REF_ROOT=/opt/opencode-ref \
bun --preload $DIFF_REF_ROOT/packages/tui/node_modules/@opentui/solid/scripts/preload.js \
    tools/diff/drivers/tui_fullscreen_ref.ts --manifest <m.json> --out <dir>
```

### Implementation notes (Step 2)

- **Fetch stub**: per-request census by query-less path; scenario `fetch` map
  served verbatim (`status`/`body`); `/api/event` (sse body) handed to the
  fixture's own `createEventStream` (identical `server.connected` initial);
  uncovered paths → recorded in `stub_gaps` AND served 503 (never silent) —
  exit code 1 if any gap.
- **Keys**: full SCHEMA 键名表 implemented; `ctrl+<a-z>`/`leader`/`alt+arrows`
  go through `pressKey` kitty modifiers (raw control bytes would be
  mis-reported as bare codepoints under the kitty protocol); named keys map to
  their SCHEMA byte strings; `raw:<literal>` feeds the literal; unknown tokens
  are a hard error. Smoke scenarios carry `keys: []`.
- **Rasterization**: per-span style → per-cell; wide chars get a same-style
  blank follower cell; combining marks join the previous cell; missing cells =
  `" "` + theme-default fg/bg; `default` color intent → theme defaults;
  `indexed` → xterm-256 palette RGB; unresolvable → `[-1,-1,-1]`.
- **Theme defaults** resolved at runtime through the ref's own loader:
  `resolveThemeDocument(getOpenCodeTheme(), "dark")` → fg `[238,238,238]`,
  bg `[10,10,10]` (v2-dark default tokens).
- **attrs**: canonical order `bold, dim, italic, underline, strikethrough,
  reverse`; out-of-set bits emitted as `unknown:16` / `unknown:64` ascending.
- **Determinism**: `config:{animations:false}`, `Math.random = () => 0` seeded
  per mount (home placeholder picker, same fix as tui-dump `home.tsx`), fixed
  scenario timestamps; `request_census` keys sorted for byte stability.

## Corpus fixes (T1 fixture bugs found by this driver — flagged for controller)

Both session scenarios crashed the reference app on first run:

1. **User-message wire shape**: corpus used `payload:{text}` + `delivery`
   (pre-v2.0.24 shape); the v2.0.24 wire schema (`Session.Message.User`) and
   every reference test fixture use top-level `text`. → Fixed in
   `session-text.json` / `session-markdown.json`.
2. **Persisted assistant text empty**: the scenarios' own notes say "the fetch
   stubs carry the persisted turns", but the `/message` stub's assistant text
   part was `""` — the full session route renders from the server message
   list, so the reply never appeared and `session.text.ended` events do not
   backfill it. → Stub content now carries the final `text.ended` value
   (identical on both harness sides; the events still replay the stream).

## Verification

- **Step 3 (double-run byte determinism): PASS** — two full runs, `cmp`
  byte-identical on all 6 grids (no spinner/clock leakage; the visible "2ms"
  step duration derives from fixed scenario timestamps).
- **Step 4: PASS** — 6/6 grids (3 scenarios × 80×24/120×30) valid under
  `python3 -m json.tool`, dense H×W cells, schema/attrs/rgb invariants
  checked, `stub_gaps` empty everywhere. Home renders the wordmark +
  placeholder (seeded entry 0); session-text renders user bubble + assistant
  reply; session-markdown renders heading/bullets/fence.
- Logs: `/tmp/opencode/logs/t2-run1.log`, `t2-run2.log`; outputs under
  `/tmp/opencode/t2/run{1,2}/`.
- Dev manifest (mirrored 120×30 tier, id-sorted, repo-root-relative paths):
  `/tmp/opencode/t2/manifest.json`.

## Step 5

Commit: `feat(diff-tui): ref headless driver over createAppFixture`

## Concerns for the controller

- The two corpus JSON edits above change T1 artifacts shared with T4 —
  T4 must build against the corrected files (they are strictly closer to the
  reference wire format; both harnesses read the same files).
- Manifest output filenames are `<case-id>.json` (e.g.
  `session-text@80x24.json`); T3/T4 comparators should confirm this
  convention.
- `ctrl+<a-z>`/`alt+<arrow>` tokens are encoded via kitty modifiers rather
  than the literal control bytes of the SCHEMA table (kitty-protocol
  equivalence; raw 0x01–0x1a bytes would be mis-parsed under kittyKeyboard).
  No smoke scenario exercises keys yet — worth one keyed scenario later.
- `/api/config` bare-array quirk and the 21-path census held: home requests 20
  paths at rest, session routes 27; zero gaps in all 6 cases.
