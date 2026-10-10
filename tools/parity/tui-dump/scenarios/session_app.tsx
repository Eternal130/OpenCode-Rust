/**
 * Full-app session-route goldens (gate-8 finding 2 second export wave).
 *
 * Boots the entire reference app headless exactly like its own lifecycle
 * tests do (`createTestRenderer` + `run()` from `src/app` against a
 * `Bun.serve` fixture server, `test/fixture/app.ts` pattern) with fixed
 * session + message data, then captures frames:
 *
 * - message-groups       — consecutive user/assistant turns, exploration +
 *                          reasoning group headers, the boundary row between
 *                          turns (80 + 40)
 * - vscroll-{top,middle,bottom,stick-bottom} — one transcript ≫ viewport
 *                          pinned at four scroll offsets (gate-9 addition a);
 *                          the manifest records the offset per scenario
 * - sidebar-expanded / sidebar-collapsed — the right-hand session sidebar
 *                          pane present vs hidden at width 160 (the pane is
 *                          `auto` only above 120 columns)
 * - session-screen-40    — the complete session route (transcript list +
 *                          composer + tabs bar) at 40 columns
 * - toast-over-session   — one toast surface over live session content
 *                          (triggered through the app's own `tui.toast.show`
 *                          event channel)
 *
 * All time-derived labels (turn durations, group durations) come from fixed
 * `time.created`/`time.completed` values; animations are off; toasts use a
 * long duration so the surface cannot expire mid-capture.
 */
/** @jsxImportSource @opentui/solid */
import { createTestRenderer } from "@opentui/core/testing"
import { ScrollBoxRenderable, type Renderable } from "@opentui/core"
import { Effect, FileSystem } from "effect"
import { Global } from "@opencode/util/global"
import { mkdir } from "node:fs/promises"
import path from "node:path"
import type { SessionMessageInfo } from "@opencode/client"
import { createEventStream, createFetch, directory, json } from "/tmp/opencode/ref/packages/tui/test/fixture/tui-client"
import { tmpdir } from "/tmp/opencode/ref/packages/tui/test/fixture/fixture"
import type { Scenario, ScenarioVariant } from "../scenario"

const MODEL = { providerID: "fixture", id: "fixture" }

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

function user(id: string, text: string, created: number): SessionMessageInfo {
  return { type: "user", id, text, time: { created } } as SessionMessageInfo
}

function assistant(id: string, created: number, completed: number, content: any[]): SessionMessageInfo {
  return {
    type: "assistant",
    id,
    agent: "build",
    model: MODEL,
    finish: "stop",
    time: { created, completed },
    content,
  } as SessionMessageInfo
}

function tool(id: string, name: string, input: Record<string, string>, created: number) {
  return {
    type: "tool" as const,
    id,
    name,
    time: { created, completed: created },
    state: {
      status: "completed" as const,
      input,
      content: [{ type: "text" as const, text: "ok" }] as [{ type: "text"; text: string }],
      metadata: {},
    },
  }
}

/** message-groups fixture: turn → tools (exploration group) → turn → reasoning + text. */
const GROUP_MESSAGES: SessionMessageInfo[] = [
  user("grp-user-0", "What does the parser do?", 0),
  assistant("grp-answer-0", 1, 2, [
    tool("grp-tool-read", "read", { path: "src/parser.ts" }, 1),
    { type: "text" as const, text: "It reads the configuration and produces a build plan." },
  ]),
  user("grp-user-1", "Now summarize the failure modes", 10),
  assistant("grp-answer-1", 11, 14, [
    { type: "reasoning" as const, text: "**Failure modes**\n\nConsider recursion limits first", time: { created: 11, completed: 12 } },
    { type: "text" as const, text: "Three failure modes dominate: recursion limits, cycle detection and budget exhaustion." },
  ]),
]

/**
 * Settle condition for GROUP_MESSAGES mounts (the message-groups
 * double-mount flake, G11-F4): the USER rows paint before the assistant
 * TEXT parts (their renderable resolves on a later task), so waiting for
 * "failure modes" (the last user row) can capture the blank pre-paint
 * state — byte-unstable across mounts under load. Wait for BOTH
 * assistant texts (fragments chosen to survive the w40 wrap) so the
 * capture only happens once the deferred rows have painted.
 */
async function settleGroups(setup: Awaited<ReturnType<typeof createTestRenderer>>) {
  await setup.waitForFrame(
    (frame) =>
      frame.includes("It reads the configuration") &&
      frame.includes("Three failure modes dominate") &&
      frame.includes("failure modes"),
  )
  await setup.waitForVisualIdle()
  await setup.renderOnce()
}

/** vscroll fixture: ten short turns, ~6 rows each — transcript ≫ a 24-row viewport. */
const VSCROLL_MESSAGES: SessionMessageInfo[] = Array.from({ length: 10 }, (_, index) => [
  user(`vs-user-${index}`, `Turn ${index} asks about section ${index} of the document`, index * 10),
  assistant(`vs-answer-${index}`, index * 10 + 1, index * 10 + 2, [
    { type: "text" as const, text: `Answer ${index}: the section describes step ${index} of the pipeline in detail.` },
  ]),
]).flat()

type RawConfig = Record<string, unknown>

interface AppFixtureInput {
  variant: ScenarioVariant
  config?: RawConfig
  sessionID: string
  title: string
  messages: SessionMessageInfo[]
  drive?: (setup: Awaited<ReturnType<typeof createTestRenderer>>, events: ReturnType<typeof createEventStream>) => Promise<void>
}

async function mountSessionApp(input: AppFixtureInput) {
  const root = await tmpdir()
  await mkdir(path.join(root.path, "state"), { recursive: true })
  const setup = await createTestRenderer({
    width: input.variant.width,
    height: input.variant.height,
    useThread: false,
    kittyKeyboard: true,
  })
  setup.renderer.start()
  const info = sessionInfo(input.sessionID, input.title)
  const events = createEventStream()
  const calls = createFetch((url) => {
    if (url.pathname === "/api/session") return json({ data: [info], cursor: {} })
    if (url.pathname === `/api/session/${input.sessionID}`) return json({ data: info })
    if (url.pathname === `/api/session/${input.sessionID}/message`)
      return json({ data: input.messages.toReversed(), cursor: {} })
    if (url.pathname.endsWith("/inbox") || url.pathname.endsWith("/permission")) return json({ data: [] })
    return undefined
  }, events)
  const server = Bun.serve({ port: 0, idleTimeout: 0, fetch: (request) => calls.fetch(request) })
  const { run } = await import("/tmp/opencode/ref/packages/tui/src/app")
  const task = Effect.runPromise(
    run({
      app: { name: "test", version: "test", channel: "test" },
      server: { endpoint: { url: server.url.toString() } },
      config: {
        get: async () => ({ animations: false, tabs: { mode: "off" }, ...input.config }),
        update: async () => ({}),
      },
      packages: { prepare: async () => ({ directory: "" }) },
      args: { sessionID: input.sessionID },
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
      await server.stop()
      // Let post-teardown persistence (session tabs storage) settle before
      // the state tmpdir disappears — avoids flock "lock compromised" noise.
      await Bun.sleep(80)
      await root[Symbol.asyncDispose]()
    }
  }
  try {
    await input.drive?.(setup, events)
  } catch (error) {
    await dispose()
    throw error
  }
  return { ...setup, dispose }
}

function findScrollBox(root: Renderable, marker: string): ScrollBoxRenderable | undefined {
  if (root instanceof ScrollBoxRenderable && root.getRenderable(marker)) return root
  for (const child of root.getChildren()) {
    const found = findScrollBox(child, marker)
    if (found) return found
  }
  return undefined
}

/** Wait until the transcript settled, then pin the scroll offset. */
async function pinScroll(
  setup: Awaited<ReturnType<typeof createTestRenderer>>,
  marker: string,
  where: "top" | "middle" | "bottom",
) {
  const scroll = findScrollBox(setup.renderer.root, marker)
  if (!scroll) throw new Error(`transcript scrollbox with ${marker} not found`)
  const maximum = Math.max(0, scroll.scrollHeight - scroll.viewport.height)
  const target = where === "top" ? 0 : where === "middle" ? Math.floor(maximum / 2) : maximum
  scroll.scrollTo(target)
  // The away-from-bottom indicator resolves on a macrotask; let it settle.
  await Bun.sleep(150)
  await setup.waitForVisualIdle()
  await setup.renderOnce()
}

async function mountGroups(variant: ScenarioVariant) {
  const app = await mountSessionApp({
    variant,
    sessionID: "ses_groups",
    title: "Message groups",
    messages: GROUP_MESSAGES,
    drive: async (setup) => {
      await settleGroups(setup)
    },
  })
  return app
}

async function mountVscroll(where: "top" | "middle" | "bottom" | "stick-to-bottom", variant: ScenarioVariant) {
  const append = where === "stick-to-bottom"
  const app = await mountSessionApp({
    variant,
    sessionID: "ses_vscroll",
    title: "Virtual scroll",
    messages: VSCROLL_MESSAGES,
    drive: async (setup, events) => {
      await setup.waitForFrame((frame) => frame.includes("Answer 9"))
      await setup.waitForVisualIdle()
      if (append) {
        // Sticky-at-bottom: a new assistant turn arrives and the viewport
        // follows it — the appended row must be visible without scrolling.
        events.emit({
          id: "evt_vs_step_started",
          created: 101,
          type: "session.step.started",
          durable: { aggregateID: "ses_vscroll", seq: 1, version: 1 },
          data: { sessionID: "ses_vscroll", assistantMessageID: "vs-answer-appended", agent: "build", model: MODEL, started: 101 },
        })
        events.emit({
          id: "evt_vs_text_started",
          created: 101,
          type: "session.text.started",
          durable: { aggregateID: "ses_vscroll", seq: 2, version: 1 },
          data: { sessionID: "ses_vscroll", assistantMessageID: "vs-answer-appended" },
        })
        events.emit({
          id: "evt_vs_text_ended",
          created: 102,
          type: "session.text.ended",
          durable: { aggregateID: "ses_vscroll", seq: 3, version: 1 },
          data: { sessionID: "ses_vscroll", assistantMessageID: "vs-answer-appended", ordinal: 0, text: "Appended while pinned to the bottom." },
        })
        events.emit({
          id: "evt_vs_step_ended",
          created: 103,
          type: "session.step.ended",
          durable: { aggregateID: "ses_vscroll", seq: 4, version: 1 },
          data: {
            sessionID: "ses_vscroll",
            assistantMessageID: "vs-answer-appended",
            finish: "stop",
            cost: 0,
            tokens: { input: 0, output: 0, reasoning: 0, cache: { read: 0, write: 0 } },
          },
        })
        await setup.waitForFrame((frame) => frame.includes("Appended while pinned"))
        await setup.waitForVisualIdle()
        await setup.renderOnce()
        return
      }
      await pinScroll(setup, "vs-user-0", where)
    },
  })
  return app
}

async function mountSidebar(sidebar: "auto" | "hide", variant: ScenarioVariant) {
  const app = await mountSessionApp({
    variant,
    sessionID: "ses_sidebar",
    title: "Sidebar fixture",
    messages: GROUP_MESSAGES,
    config: { session: { sidebar } },
    drive: async (setup) => {
      // With the pane hidden the session title never renders (it lives in
      // the sidebar), so only the transcript anchors the wait.
      await settleGroups(setup)
    },
  })
  return app
}

async function mountScreen40(variant: ScenarioVariant) {
  const app = await mountSessionApp({
    variant,
    sessionID: "ses_narrow",
    title: "Narrow session",
    messages: GROUP_MESSAGES,
    config: { tabs: { mode: "on" } },
    drive: async (setup) => {
      await settleGroups(setup)
    },
  })
  return app
}

async function mountToast(variant: ScenarioVariant) {
  const app = await mountSessionApp({
    variant,
    sessionID: "ses_toast",
    title: "Toast fixture",
    messages: GROUP_MESSAGES,
    drive: async (setup, events) => {
      await settleGroups(setup)
      events.emit({
        id: "evt_toast_show",
        created: 200,
        type: "tui.toast.show",
        location: { directory },
        data: { title: "Fixture notice", message: "Toast rendered over session content", variant: "info", duration: 60000 },
      })
      await setup.waitForFrame((frame) => frame.includes("Toast rendered over session content"))
      await setup.renderOnce()
    },
  })
  return app
}

function scenario(
  id: string,
  description: string,
  componentClass: string,
  variants: ScenarioVariant[],
  mount: (variant: ScenarioVariant) => Promise<unknown>,
  options: { scrollOffset?: Scenario["scrollOffset"]; tier?: Scenario["tier"] } = {},
): Scenario {
  return {
    id,
    source: "packages/tui/src/app.tsx full-app fixture (test/fixture/app.ts pattern)",
    description,
    componentClass,
    tier: options.tier ?? "tier-a",
    variants,
    scrollOffset: options.scrollOffset,
    mount: mount as Scenario["mount"],
  }
}

export const scenarios: Scenario[] = [
  scenario(
    "message-groups",
    "Consecutive user/assistant turns with exploration ('Explored: 1 read') and reasoning ('Thought') group headers plus the boundary row between turns",
    "message-group",
    [
      { width: 80, height: 30 },
      { width: 40, height: 30 },
    ],
    mountGroups,
  ),
  scenario(
    "vscroll-top",
    "Virtual-scroll transcript ≫ viewport pinned at scrollTop=0 (oldest turn at the top, 'Jump to latest' indicator visible)",
    "vscroll",
    [{ width: 80, height: 24 }],
    (variant) => mountVscroll("top", variant),
    { scrollOffset: "top" },
  ),
  scenario(
    "vscroll-middle",
    "Same transcript pinned at the middle of the scroll range (half the maximum scrollTop)",
    "vscroll",
    [{ width: 80, height: 24 }],
    (variant) => mountVscroll("middle", variant),
    { scrollOffset: "middle" },
  ),
  scenario(
    "vscroll-bottom",
    "Same transcript scrolled to the maximum scrollTop after being away (indicator cleared, sticky re-armed)",
    "vscroll",
    [{ width: 80, height: 24 }],
    (variant) => mountVscroll("bottom", variant),
    { scrollOffset: "bottom" },
  ),
  scenario(
    "vscroll-stick-bottom",
    "Same transcript left sticky-at-bottom while a new assistant turn streams in — the viewport follows the append",
    "vscroll",
    [{ width: 80, height: 24 }],
    (variant) => mountVscroll("stick-to-bottom", variant),
    { scrollOffset: "stick-to-bottom" },
  ),
  scenario(
    "sidebar-expanded",
    "Session route at width 160 with the right-hand sidebar pane visible (session title + plugin slot area, sidebar=auto)",
    "sidebar",
    [{ width: 160, height: 30 }],
    (variant) => mountSidebar("auto", variant),
  ),
  scenario(
    "sidebar-collapsed",
    "Same session route with the sidebar pane hidden (session.sidebar=hide) — full-width transcript",
    "sidebar",
    [{ width: 160, height: 30 }],
    (variant) => mountSidebar("hide", variant),
  ),
  scenario(
    "session-screen-40",
    "Complete session route at 40 columns: transcript list, composer input and the horizontal tabs bar",
    "session-screen",
    [{ width: 40, height: 30 }],
    mountScreen40,
  ),
  scenario(
    "toast-over-session",
    "One toast surface (title + message + dismiss affordance) rendered over live session content via the app's tui.toast.show event",
    "toast",
    [{ width: 80, height: 30 }],
    mountToast,
    { tier: "tier-c1" },
  ),
]
