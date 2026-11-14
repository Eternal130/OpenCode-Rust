/**
 * Channel 8 (TUI full-screen diff) — Task 2: reference (bun) headless driver.
 *
 * Feeds `tui-diff-scenario/1` fixtures (tools/diff/corpus/tui/SCHEMA.md) to
 * the reference v2.0.24 TUI booted headless through its own fixture harness
 * and exports the resting frame as a normalized dense cell grid
 * (`tui-diff-grid/1`).
 *
 * FIXTURE ASSEMBLY: direct import of the reference `createAppFixture`
 * (`$DIFF_REF_ROOT/packages/tui/test/fixture/app.ts`) — the primary option
 * from the task brief; the ~50-line in-driver re-assembly fallback was NOT
 * needed. (Rationale: the fixture file's own bare imports — @opencode/util,
 * effect, @opentui/core/testing — resolve against the reference tree's
 * node_modules, which is intact at DIFF_REF_ROOT.)
 *
 * STAGING MAPPING (Step 1 — mirrors tools/parity/tui-dump/, see README.md):
 *   tui-dump                                     this driver
 *   ————————————————————————————————————————————————————————————————
 *   runtime copy at /tmp/opencode/tui-dump       tools/diff/drivers/ (repo,
 *                                                later the orchestrator's
 *                                                STAGE dir)
 *   OPENTUI_REF_ROOT=/tmp/opencode/ref           DIFF_REF_ROOT (default
 *                                                ./ref), read-only
 *   link_modules.ts node_modules symlinks        NOT needed: we never import
 *                                                bare @opentui/* packages
 *                                                ourselves — the ref app and
 *                                                its fixture resolve their own
 *                                                deps from the ref tree; the
 *                                                solid transform preload is
 *                                                passed as an ABSOLUTE file
 *                                                path (see Run below)
 *   `bun --preload @opentui/solid/preload`       `bun --preload
 *                                              $DIFF_REF_ROOT/packages/tui/
 *                                              node_modules/@opentui/solid/
 *                                              scripts/preload.js`
 *   bun 1.3.10 (tui-dump invocation convention)  same environment bun
 *                                                (1.3.10, controller ruling)
 *
 * Run (dev, cwd = repo root):
 *   DIFF_REF_ROOT=./ref \
 *   bun --preload $DIFF_REF_ROOT/packages/tui/node_modules/@opentui/solid/scripts/preload.js \
 *       tools/diff/drivers/tui_fullscreen_ref.ts \
 *       --manifest <manifest.json> --out <dir>
 *
 * Manifest schema (controller-pinned, "tui-diff-manifest/1"):
 *   {"schema":"tui-diff-manifest/1","cases":[
 *     {"id":"<scenario-id>@<W>x<H>","scenario":"<path relative to cwd>",
 *      "width":W,"height":H}, ...]}   // cases sorted by id
 *
 * Output: one `<case-id>.json` per case under --out:
 *   {"schema":"tui-diff-grid/1","id","width","height",
 *    "cells":[[{"ch","attrs","fg","bg"}...]],   // dense H×W
 *    "stub_gaps":[...paths...], "request_census":{path:count}}
 *
 * Determinism discipline (SCHEMA §5 / task brief Step 3):
 *   - config { animations: false } (fixture default, passed explicitly);
 *   - `Math.random` seeded to () => 0 for the whole mount and restored after
 *     (the home route's Prompt placeholder picks via Math.random — same fix
 *     as tools/parity/tui-dump/scenarios/home.tsx);
 *   - all scenario data (incl. timestamps) comes from the fixed fetch/event
 *     stubs; no wall-clock fields are expected to leak (verified by the
 *     double-run byte-compare gate).
 */
import { existsSync } from "node:fs"
import { mkdirSync, readFileSync, writeFileSync } from "node:fs"
import path from "node:path"

const REF_ROOT = process.env.DIFF_REF_ROOT ?? "./ref"
const REF_APP_FIXTURE = path.join(REF_ROOT, "packages/tui/test/fixture/app.ts")
const REF_THEME_INDEX = path.join(REF_ROOT, "packages/tui/src/theme/index.ts")

// ---------------------------------------------------------------------------
// key vocabulary — SCHEMA.md 键名表 (source of truth: tools/parity/src/tui_diff.rs encode_key)
// mapped onto mockInput.pressKey (kitty protocol: the test renderer is created
// with kittyKeyboard: true). Modifiers are encoded natively by the mock-keys
// kitty path; tokens whose bytes have no kitty name (pageup/pagedown/alt+arrows
// raw sequences) fall through to the legacy byte path of pressKey, which emits
// the exact SCHEMA bytes.
// ---------------------------------------------------------------------------

const NAMED_KEYS: Record<string, string> = {
  enter: "\r",
  return: "\r",
  esc: "\x1b",
  escape: "\x1b",
  tab: "\t",
  backspace: "\x7f",
  delete: "\x1b[3~",
  up: "\x1b[A",
  down: "\x1b[B",
  right: "\x1b[C",
  left: "\x1b[D",
  home: "\x1b[H",
  end: "\x1b[F",
  pageup: "\x1b[5~",
  pagedown: "\x1b[6~",
  space: " ",
}

function pressToken(mockInput: any, token: string) {
  if (token.startsWith("raw:")) {
    mockInput.pressKey(token.slice(4))
    return
  }
  const ctrl = /^ctrl\+([a-z])$/.exec(token)
  if (ctrl) {
    // SCHEMA: 0x01..0x1a — the kitty path encodes the ctrl modifier natively,
    // which the reference keybinds match; the raw control byte would be
    // reported as a bare codepoint under the kitty protocol.
    mockInput.pressKey(ctrl[1]!, { ctrl: true })
    return
  }
  // W3 chord adjudication: `leader` is v2's default `ctrl+x` keybind
  // (config/keybind.ts:41 LeaderDefault), matched through the kitty key
  // matcher (keymap.tsx:190 createKeyMatcher — key + modifier). A bare
  // \x18 byte parses as codepoint 24 WITHOUT the ctrl modifier under the
  // kitty protocol, so it must not arm the chord; encode it exactly like
  // the ctrl tokens above.
  if (token === "leader") {
    mockInput.pressKey("x", { ctrl: true })
    return
  }
  const alt = /^alt\+(up|down|right|left)$/.exec(token)
  if (alt) {
    mockInput.pressKey(`ARROW_${alt[1]!.toUpperCase()}` as never, { meta: true })
    return
  }
  const named = NAMED_KEYS[token]
  if (named === undefined) throw new Error(`unknown key token: ${token}`)
  mockInput.pressKey(named)
}

// ---------------------------------------------------------------------------
// cell attribute mapping (bit layout: @opentui/core, cf. tui-dump/frame.ts)
// ---------------------------------------------------------------------------

const ATTR_BOLD = 1 << 0
const ATTR_DIM = 1 << 1
const ATTR_ITALIC = 1 << 2
const ATTR_UNDERLINE = 1 << 3
const ATTR_BLINK = 1 << 4
const ATTR_INVERSE = 1 << 5
const ATTR_HIDDEN = 1 << 6
const ATTR_STRIKE = 1 << 7

/** canonical set (task brief): emitted in this order, then unknown bits ascending */
const CANONICAL_ATTRS: [number, string][] = [
  [ATTR_BOLD, "bold"],
  [ATTR_DIM, "dim"],
  [ATTR_ITALIC, "italic"],
  [ATTR_UNDERLINE, "underline"],
  [ATTR_STRIKE, "strikethrough"],
  [ATTR_INVERSE, "reverse"],
]

function attrList(attrs: number): string[] {
  const out: string[] = []
  for (const [bit, name] of CANONICAL_ATTRS) if (attrs & bit) out.push(name)
  for (const bit of [ATTR_BLINK, ATTR_HIDDEN]) if (attrs & bit) out.push(`unknown:${bit}`)
  return out
}

// ---------------------------------------------------------------------------
// color mapping
// ---------------------------------------------------------------------------

const SENTINEL = [-1, -1, -1]

/**
 * indexed→RGB table of the reference `v1-migrate.ts` `ansi()` (lines 470-495):
 * 0x80-based base colors, color cube `part === 0 ? 0 : part * 40 + 55`,
 * grayscale ramp `(i - 232) * 10 + 8`. Controller ruling: both harnesses use
 * THIS table (mirrored in the rust driver
 * crates/oc-tui/tests/tui_fullscreen_diff_driver.rs `indexed_to_rgb`).
 */
function indexedToRgb(index: number): number[] {
  const i = index & 0xff
  if (i < 16) {
    const table = [
      [0x00, 0x00, 0x00], [0x80, 0x00, 0x00], [0x00, 0x80, 0x00], [0x80, 0x80, 0x00],
      [0x00, 0x00, 0x80], [0x80, 0x00, 0x80], [0x00, 0x80, 0x80], [0xc0, 0xc0, 0xc0],
      [0x80, 0x80, 0x80], [0xff, 0x00, 0x00], [0x00, 0xff, 0x00], [0xff, 0xff, 0x00],
      [0x00, 0x00, 0xff], [0xff, 0x00, 0xff], [0x00, 0xff, 0xff], [0xff, 0xff, 0xff],
    ]
    return table[i] ?? SENTINEL
  }
  if (i < 232) {
    const n = i - 16
    const value = (part: number) => (part === 0 ? 0 : part * 40 + 55)
    return [value(Math.floor(n / 36)), value(Math.floor(n / 6) % 6), value(n % 6)]
  }
  const g = (i - 232) * 10 + 8
  return [g, g, g]
}

function colorToRgb(color: any, themeDefault: number[]): number[] {
  if (!color) return themeDefault
  try {
    const intent = color.intent ?? "rgb"
    if (intent === "default") return themeDefault
    if (intent === "indexed") return indexedToRgb(color.slot ?? 0)
    const [r, g, b] = color.toInts()
    if ([r, g, b].some((v) => typeof v !== "number" || v < 0 || v > 255)) return SENTINEL
    return [r, g, b]
  } catch {
    return SENTINEL
  }
}

// ---------------------------------------------------------------------------
// display width (minimal grapheme handling; smoke corpus is ASCII/markdown —
// wide chars get a trailing blank cell of the same style, combining marks
// join the previous cell)
// ---------------------------------------------------------------------------

const WIDE = /[\u1100-\u115F\u2E80-\uA4CF\uA960-\uA97F\uAC00-\uD7A3\uF900-\uFAFF\uFE10-\uFE19\uFE30-\uFE6F\uFF00-\uFF60\uFFE0-\uFFE6\u{1F300}-\u{1FAFF}\u{20000}-\u{3FFFD}]/u
const COMBINING = /[\u0300-\u036F\u1AB0-\u1AFF\u1DC0-\u1DFF\u20D0-\u20FF\uFE20-\uFE2F]/

function charWidth(ch: string): number {
  if (COMBINING.test(ch)) return 0
  if (WIDE.test(ch)) return 2
  return 1
}

// ---------------------------------------------------------------------------
// scenarios / manifest
// ---------------------------------------------------------------------------

interface ScenarioFixture {
  schema: string
  id: string
  width: number
  height: number
  fetch: Record<string, { status: number; body: unknown }>
  events: unknown[]
  keys: string[]
  settle_ms: number
  anim?: {
    ticks: number
    animations?: boolean
    tick_events?: unknown[][]
  }
}

interface ManifestCase {
  id: string
  scenario: string
  width: number
  height: number
  /** B2: staged plugin package dirs (absolute; the orchestrator's
   * payload registry staged them into STAGE/plugins/node_modules —
   * the read-only cache entities are never loaded in place). */
  plugins?: string[]
}

interface Manifest {
  schema: string
  cases: ManifestCase[]
}

interface GridCell {
  ch: string
  attrs: string[]
  fg: number[]
  bg: number[]
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

async function main() {
  const args = process.argv.slice(2)
  const flag = (name: string) => {
    const i = args.indexOf(name)
    return i >= 0 ? args[i + 1] : undefined
  }
  const manifestPath = flag("--manifest")
  const outDir = flag("--out")
  if (!manifestPath || !outDir) {
    console.error("usage: tui_fullscreen_ref.ts --manifest <file> --out <dir>")
    process.exit(2)
  }

  const manifest = JSON.parse(readFileSync(manifestPath, "utf8")) as Manifest
  if (manifest.schema !== "tui-diff-manifest/1") throw new Error(`unknown manifest schema: ${manifest.schema}`)
  const cases = [...manifest.cases].sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0))

  // theme defaults: the pinned corpus theme (opencode v2, dark) resolved via
  // the reference's own theme loader — default fg/bg tokens (SCHEMA 主题 pinning).
  const themeIndex: any = await import(REF_THEME_INDEX)
  const tuiTheme: any = await import(path.join(REF_ROOT, "packages/theme/src/tui/index.ts"))
  const resolved = tuiTheme.resolveThemeDocument(themeIndex.getOpenCodeTheme(), "dark")
  const THEME_FG = resolved.text.base.toInts().slice(0, 3)
  const THEME_BG = resolved.background.base.toInts().slice(0, 3)

  const { createAppFixture }: any = await import(REF_APP_FIXTURE)
  mkdirSync(outDir, { recursive: true })
  const summary: Record<string, unknown>[] = []
  let anyGap = false

  for (const c of cases) {
    const scenarioPath = path.resolve(process.cwd(), c.scenario)
    const scenario = JSON.parse(readFileSync(scenarioPath, "utf8")) as ScenarioFixture
    if (scenario.schema !== "tui-diff-scenario/1") throw new Error(`${c.id}: bad scenario schema`)

    // session-route convention (SCHEMA): boot into the session id from the
    // /api/session list + /api/session/active stubs.
    const args2: Record<string, unknown> = {}
    const active = (scenario.fetch["/api/session/active"]?.body as any)?.data
    const list = (scenario.fetch["/api/session"]?.body as any)?.data
    const sessionID = active?.id ?? (Array.isArray(list) ? list[0]?.id : undefined)
    if (sessionID) args2.sessionID = sessionID

    const census = new Map<string, number>()
    const gaps = new Set<string>()

    const fetchHandler = (url: URL) => {
      census.set(url.pathname, (census.get(url.pathname) ?? 0) + 1)
      const entry = scenario.fetch[url.pathname]
      if (entry) {
        // /api/event is the SSE stream: served by the fixture's own
        // createEventStream (initial server.connected, identical to the
        // scenario's initial); the scenario `events` are then emitted via
        // events.emit below (task brief Step 2).
        if (url.pathname === "/api/event" && (entry.body as any)?.sse) return undefined
        // C3: a 204 No Content must carry NO body — a real 204 never
        // has one, and bun's fetch rejects a bodied 204 (the interrupt
        // stub then rejects unhandled and the renderer console panel
        // flips open — a mock artifact, not a diff).
        const body = entry.status === 204 ? null : JSON.stringify(entry.body)
        return new Response(body, {
          status: entry.status,
          headers: { "content-type": "application/json" },
        })
      }
      if (url.pathname === "/api/event") return undefined
      gaps.add(url.pathname)
      return new Response(JSON.stringify({ error: "stub_gap", path: url.pathname }), {
        status: 503,
        headers: { "content-type": "application/json" },
      })
    }

    // determinism: home placeholder Math.random seed (see file header)
    const randomBefore = Math.random
    Math.random = () => 0
    // A2: anim scenarios boot through the stock fixture too — the
    // probed ManualClock path (createTestRenderer clock injection) is
    // NATIVELY UNSTABLE against the full app: boot deadlocks on a
    // frozen clock (boot timers ride the renderer clock); driving the
    // clock by real time boots fine but post-event captures crash in
    // native buffer reads (segfault in bufferGetCharPtr/getRealCharBytes
    // — see .superpowers/sdd/diff-ext/a2-report.md §ref-clock). The
    // deterministic comparable surface is therefore the stock
    // animations:false fixture: ref frames are event-driven per tick
    // (tick_events), while the rust side steps its virtual clock
    // (advance_clock k·ANIM_TICK_MS) — clock-driven rust animation vs
    // the ref's animation-gated static shapes is a REGISTERED harness
    // artifact family (CAUSES), not silent.
    let app: any
    try {
      // B2 real-plugin scenarios: boot through the SAME seam the ref
      // plugin-keymap.test.tsx precedent uses — createAppFixture's
      // config.plugins directory entries. Boot success/failure is the
      // fixture's own decision (probed: 2.0.24's Host.resolve throws
      // on directory entries lacking a root ./server file — the
      // deterministic "Plugin failed" state is itself diff surface).
      const plugins = c.plugins ?? []
      const pluginsUnavailable =
        plugins.length > 0 && plugins.some((dir) => !existsSync(dir))
      app = await createAppFixture({
        width: c.width,
        height: c.height,
        config: { animations: false, ...(plugins.length > 0 ? { plugins } : {}) },
        args: args2,
        fetch: fetchHandler,
      })
      // A2 frame-sequence mode (SCHEMA `anim`): the ref side's frames
      // are EVENT-driven — tick_events[k] injected, settle, capture
      // (no virtual clock; see the fixture note above). The rust side
      // additionally steps its store clock k·100ms per tick
      // (ANIM_TICK_MS, SCHEMA constant); clock-driven shape
      // differences are the registered harness artifact family.
      const isAnim = scenario.anim !== undefined
      const ticks = scenario.anim?.ticks ?? 8
      const tickEvents = scenario.anim?.tick_events ?? []
      await app.ready
      await app.waitForVisualIdle({ quietFrames: 3, maxFrames: 600 })
      for (const event of scenario.events) app.events.emit(event)
      // B2-fix: plugin scenarios insert a per-token idle between key
      // tokens — the palette's search input mounts asynchronously after
      // ctrl+p and chars pressed in the same synchronous burst are
      // dropped (observed: an empty search box where the rust side's
      // sequential press() state machine had already filtered). The
      // rust driver needs no equivalent (its press() path is a
      // synchronous state machine, no mount race). Non-plugin scenarios
      // keep the original single-pump flow — the 75-case baseline does
      // not drift.
      // C3: recorded sessions (rec-*) get the same per-token idle for
      // the WHOLE sequence — real recordings carry human pacing (the
      // recorder captures keys between real waits), while replay
      // compresses them to a zero-gap burst. The async-loaded dialogs
      // (session list's debounced resource) drop an `enter` that
      // arrives before their options mount; same fixture-artifact
      // family as B2, same ref-only remedy.
      const perTokenIdle = plugins.length > 0 || c.id.startsWith("rec-")
      for (const token of scenario.keys) {
        pressToken(app.mockInput, token)
        // C3: never idle right AFTER a leader stroke — the which-key
        // arm is same-burst; a gap drops the chord and the
        // continuation letter types into the composer instead.
        const leaderStroke = token === "ctrl+x" || token === "leader"
        if (perTokenIdle && !leaderStroke) await app.waitForVisualIdle({ quietFrames: 2, maxFrames: 200 })
      }
      await app.waitForVisualIdle({ quietFrames: 3, maxFrames: 600 })
      await Bun.sleep(scenario.settle_ms ?? 200)
      await app.renderOnce()
      const cells = rasterize(app.captureSpans(), c.width, c.height, THEME_FG, THEME_BG)
      const frames: GridCell[][][] = [cells]
      for (let k = 1; isAnim && k <= ticks; k++) {
        for (const event of tickEvents[k] ?? []) app.events.emit(event)
        // two idle+render passes per tick: propagation after a long
        // quiet stretch needs one extra pass (observed one-tick
        // content lag without it); NO settle sleep — real time spent
        // per tick must stay well under the toast's real-timer 5000ms
        // dismissal deadline (the ref toast timer is NOT
        // clock-injectable — SCHEMA comparability note)
        await app.waitForVisualIdle({ quietFrames: 3, maxFrames: 200 })
        await app.renderOnce()
        await app.waitForVisualIdle({ quietFrames: 3, maxFrames: 200 })
        await app.renderOnce()
        frames.push(rasterize(app.captureSpans(), c.width, c.height, THEME_FG, THEME_BG))
      }

      const gapsSorted = [...gaps].sort()
      const censusObj: Record<string, number> = {}
      for (const key of [...census.keys()].sort()) censusObj[key] = census.get(key)!
      const out: Record<string, unknown> = {
        schema: "tui-diff-grid/1",
        id: c.id,
        width: c.width,
        height: c.height,
        cells,
        stub_gaps: gapsSorted,
        request_census: censusObj,
      }
      if (isAnim) out.frames = frames
      if (pluginsUnavailable) out.plugins_unavailable = true
      writeFileSync(path.join(outDir, `${c.id}.json`), `${JSON.stringify(out)}\n`)
      if (gapsSorted.length > 0) anyGap = true
      summary.push({ id: c.id, gaps: gapsSorted.length, requests: census.size })
      console.log(`[tui-fullscreen-ref] ✓ ${c.id} gaps=${gapsSorted.length} paths=${census.size}`
        + (frames.length > 1 ? ` frames=${frames.length}` : ""))
    } finally {
      Math.random = randomBefore
      if (app) await app[Symbol.asyncDispose]()
    }
  }

  console.log(`[tui-fullscreen-ref] done: ${summary.length} cases${anyGap ? " (STUB GAPS PRESENT)" : ""}`)
  if (anyGap) process.exit(1)
}

// ---------------------------------------------------------------------------
// rasterization: captureSpans() → dense cell grid
// ---------------------------------------------------------------------------

function rasterize(frame: any, width: number, height: number, themeFg: number[], themeBg: number[]): GridCell[][] {
  const lines: any[] = frame.lines ?? []
  if (lines.length !== height) throw new Error(`frame rows ${lines.length} != height ${height}`)
  const grid: GridCell[][] = []
  for (let y = 0; y < height; y++) {
    const row: (GridCell | null)[] = Array(width).fill(null)
    let col = 0
    for (const span of lines[y]!.spans ?? []) {
      const attrs = attrList((span.attributes ?? 0) & 0xff)
      const fg = colorToRgb(span.fg, themeFg)
      const bg = colorToRgb(span.bg, themeBg)
      for (const ch of [...String(span.text ?? "")]) {
        const w = charWidth(ch)
        if (w === 0) {
          // combining mark joins the previous occupied cell
          const prev = col > 0 ? row[col - 1] : undefined
          if (prev) prev.ch += ch
          continue
        }
        if (col >= width) break
        row[col] = { ch, attrs, fg: [...fg], bg: [...bg] }
        if (w === 2 && col + 1 < width) row[col + 1] = { ch: " ", attrs: [...attrs], fg: [...fg], bg: [...bg] }
        col += w
      }
    }
    // missing cells: blank with theme-default colors (task brief raster rule)
    for (let x = 0; x < width; x++) {
      if (row[x] === null) row[x] = { ch: " ", attrs: [], fg: [...themeFg], bg: [...themeBg] }
    }
    grid.push(row as GridCell[])
  }
  return grid
}

await main()
