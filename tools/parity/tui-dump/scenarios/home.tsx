/**
 * Home-route golden (the Phase-8 registered chore): the full reference
 * app booted headless exactly like the session scenarios
 * (`session_app.tsx`), but WITHOUT a sessionID arg — the route lands on
 * home: the OPENCODE wordmark, the centered prompt composer (the same
 * `Prompt` the session route docks — placeholder entry pinned below),
 * and the `home.footer` plugin slot (MCP/plugin status gated by width,
 * the app version right-aligned).
 *
 * Determinism: the app's Home passes a THREE-entry placeholder list and
 * `Prompt` picks its entry with `Math.random()` — the scenario seeds
 * `Math.random = () => 0` for the whole mount (index 0, "Fix a TODO in
 * the codebase") and restores it on dispose; the exporter's double
 * mount therefore renders byte-identical frames.
 */
/** @jsxImportSource @opentui/solid */
import { createTestRenderer } from "@opentui/core/testing"
import { Effect, FileSystem } from "effect"
import { Global } from "@opencode/util/global"
import { mkdir } from "node:fs/promises"
import path from "node:path"
import { createEventStream, createFetch, directory, json } from "/tmp/opencode/ref/packages/tui/test/fixture/tui-client"
import { tmpdir } from "/tmp/opencode/ref/packages/tui/test/fixture/fixture"
import type { Scenario, ScenarioVariant } from "../scenario"

function sessionInfo(id: string, title: string) {
  return {
    id,
    title,
    projectID: "project",
    location: { directory },
    cost: 0,
    tokens: { input: 0, output: 0, reasoning: 0, cache: { read: 0, write: 0 } },
    time: { created: 0, updated: 0 },
  }
}

let activeMounts = 0
let originalRandom: (() => number) | undefined

/** Install the constant seed if needed; returns the previous value for
 * the matching restore (when the last mount's dispose finishes). */
function seededRandom(): () => number {
  if (originalRandom === undefined) {
    originalRandom = Math.random
    Math.random = () => 0
  }
  return originalRandom
}

function unseedRandom(previous: () => number) {
  if (activeMounts === 0 && originalRandom === previous) {
    Math.random = originalRandom
    originalRandom = undefined
  }
}

/** The fixture session list the app syncs on boot (home ignores it visually). */
const SESSIONS = [
  sessionInfo("ses_home_a", "Home fixture A"),
  sessionInfo("ses_home_b", "Home fixture B"),
]

async function mountHome(variant: ScenarioVariant) {
  const root = await tmpdir()
  await mkdir(path.join(root.path, "state"), { recursive: true })
  const setup = await createTestRenderer({
    width: variant.width,
    height: variant.height,
    useThread: false,
    kittyKeyboard: true,
  })
  setup.renderer.start()
  const events = createEventStream()
  const info = SESSIONS[0]
  const calls = createFetch((url) => {
    if (url.pathname === "/api/session") return json({ data: SESSIONS, cursor: {} })
    if (url.pathname === `/api/session/${info.id}`) return json({ data: info })
    if (url.pathname.endsWith("/message")) return json({ data: [], cursor: {} })
    if (url.pathname.endsWith("/inbox") || url.pathname.endsWith("/permission")) return json({ data: [] })
    return undefined
  }, events)
  const server = Bun.serve({ port: 0, idleTimeout: 0, fetch: (request) => calls.fetch(request) })
  const { run } = await import("/tmp/opencode/ref/packages/tui/src/app")
  // Pin the prompt placeholder to list entry 0 (see the file header).
  // Scoped by an outstanding-mount COUNTER: the exporter's cleanup
  // fires each dispose fire-and-forget, so a naive restore could race
  // the next mount's seed — but a process-global seed breaks the
  // OTHER full-app scenarios' cross-mount isolation (their generated
  // ids collide with the previous mount's leftovers). The counter
  // keeps the seed installed exactly while ≥1 home mount is live.
  activeMounts++
  const randomBefore = seededRandom()
  const task = Effect.runPromise(
    run({
      app: { name: "test", version: "test", channel: "test" },
      server: { endpoint: { url: server.url.toString() } },
      config: {
        get: async () => ({ animations: false, tabs: { mode: "off" } }),
        update: async () => ({}),
      },
      packages: { prepare: async () => ({ directory: "" }) },
      args: {},
      terminalHandoff: async () => ({ renderer: setup.renderer, mode: "dark", complete: () => {} }),
      log: () => {},
    }).pipe(
      Effect.provide(
        Global.layerWith({
          home: path.join(root.path, "home"),
          data: path.join(root.path, "data"),
          cache: path.join(root.path, "cache"),
          config: path.join(root.path, "config"),
          state: path.join(root.path, "state"),
          bin: path.join(root.path, "cache", "bin"),
          log: path.join(root.path, "data", "log"),
          repos: path.join(root.path, "data", "repos"),
          tmp: path.join(root.path, "tmp"),
        }),
      ),
      Effect.provide(FileSystem.layerNoop({})),
    ),
  )
  const dispose = async () => {
    try {
      if (!setup.renderer.isDestroyed) setup.renderer.destroy()
      await task
    } finally {
      activeMounts--
      unseedRandom(randomBefore)
      await server.stop(true)
      await Bun.sleep(80)
      await root[Symbol.asyncDispose]()
    }
  }
  try {
    // Wait for BOTH the prompt placeholder and the home.footer slot's
    // version label (plugin reconciliation lands async — capturing at
    // "Ask anything" alone races the footer's first frame).
    await setup.waitForFrame((frame) => frame.includes("Ask anything") && frame.includes("test"))
    await setup.waitForVisualIdle()
    await setup.renderOnce()
  } catch (error) {
    await dispose()
    throw error
  }
  return { ...setup, dispose }
}

export const scenarios: Scenario[] = [
  {
    id: "home",
    source: "packages/tui/src/routes/home.tsx full-app fixture (session_app.tsx pattern, no sessionID)",
    description: "Home route: OPENCODE wordmark, centered prompt composer (placeholder pinned to entry 0) and the home.footer plugin slot (app version)",
    componentClass: "home",
    tier: "tier-a",
    variants: [
      { width: 80, height: 30 },
      { width: 160, height: 30 },
    ],
    mount: mountHome,
  },
]
