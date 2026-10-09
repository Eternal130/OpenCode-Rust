/** @jsxImportSource @opentui/solid */
import { testRender } from "@opentui/solid"
import { createSignal, Show } from "solid-js"
import { mkdtemp } from "node:fs/promises"
import os from "node:os"
import path from "node:path"
import type { Route } from "@opencode/client"
import { ConfigProvider, type Info } from "/tmp/opencode/ref/packages/tui/src/config/index.tsx"
import { ThemeProvider, useThemes } from "/tmp/opencode/ref/packages/tui/src/context/theme"
import { StorageProvider, useStorage } from "/tmp/opencode/ref/packages/tui/src/context/storage"
import { TuiAppProvider } from "/tmp/opencode/ref/packages/tui/src/context/runtime"
import { Keymap } from "/tmp/opencode/ref/packages/tui/src/context/keymap"
import diffViewerPlugin from "/tmp/opencode/ref/packages/tui/src/feature-plugins/system/diff-viewer.tsx"
import { createTuiResolvedConfig } from "/tmp/opencode/ref/packages/tui/test/fixture/tui-runtime"
import { TestTuiContexts } from "/tmp/opencode/ref/packages/tui/test/fixture/tui-environment"
import { createApi, createEventStream, createFetch, json } from "/tmp/opencode/ref/packages/tui/test/fixture/tui-client"
import { DialogProvider, useDialog } from "/tmp/opencode/ref/packages/tui/src/ui/dialog.tsx"
import { createDialogApi } from "/tmp/opencode/ref/packages/tui/src/plugin/api"
import { ToastProvider } from "/tmp/opencode/ref/packages/tui/src/ui/toast"
import { emptyThemeSource } from "/tmp/opencode/ref/packages/tui/test/fixture/fixture"
import type { Scenario, ScenarioVariant } from "../scenario"

const startRoute: Route = { type: "session", sessionID: "session-1" }
const baseFixture = { name: "v2", ref: "refs/heads/v2", source: "default" }

const hunkDiff = [
  {
    file: "src/file.txt",
    additions: 3,
    deletions: 3,
    status: "modified",
    patch: `--- a/src/file.txt
+++ b/src/file.txt
@@ -1,3 +1,3 @@
 const first = true
-const oldFirst = true
+const newFirst = true
 const afterFirst = true
@@ -20,3 +20,3 @@
 const second = true
-const oldSecond = true
+const newSecond = true
 const afterSecond = true
@@ -40,3 +40,3 @@
 const third = true
-const oldThird = true
+const newThird = true
 const afterThird = true`,
  },
]

const session = {
  id: "session-1",
  projectID: "project-1",
  location: { directory: "/repo/session" },
  title: "Session",
  cost: { currency: "USD", amount: 0 },
  tokens: { input: 0, output: 0, reasoning: 0, cache: { read: 0, write: 0 } },
  time: { created: 0, updated: 0 },
}

async function mountDiffViewer(variant: ScenarioVariant) {
  const commands = new Map<string, { id: string; title?: string; run: () => void }>()
  const [current, setCurrent] = createSignal<Route>(startRoute)
  const [branch] = createSignal("feature")
  const [sessionStatus] = createSignal<"idle" | "running">("idle")
  const state = await mkdtemp(path.join(os.tmpdir(), "tui-dump-diff-"))
  let renderDiff: (() => unknown) | undefined
  let renderCommands: ((props: unknown) => unknown) | undefined
  const stored: { info: Info } = { info: {} }
  const config = createTuiResolvedConfig(stored.info)

  const transport = createFetch(async (url) => {
    if (url.pathname === "/api/vcs/base") return json({ location: session.location, data: baseFixture })
    if (url.pathname === "/api/vcs/branch")
      return json({
        location: session.location,
        data: ["v2", "release", "origin/release"].filter((name) => name.includes(url.searchParams.get("search") ?? "")),
      })
    if (url.pathname === "/api/session/session-1/diff") return json({ data: hunkDiff })
    if (url.pathname !== "/api/vcs/diff") return undefined
    return json({
      location: { directory: "/repo/session", project: { id: "project-1", directory: "/repo/session" } },
      data: hunkDiff,
    })
  }, createEventStream())

  function Harness() {
    let theme: ReturnType<ReturnType<typeof useThemes>["currentTokens"]>
    function Content() {
      const dialog = useDialog()
      const keymap = Keymap.use()
      const shortcuts = Keymap.useShortcuts()
      theme = useThemes().currentTokens()
      const pluginContext = {
        options: {},
        storage: useStorage(),
        client: createApi(transport.fetch),
        data: {
          session: { get: () => ({ ...session, location: session.location }), status: sessionStatus },
          location: {
            default: () => ({ directory: "/repo/default" }),
            vcs: { info: () => ({ branch: { current: branch() } }) },
          },
        },
        get theme() {
          return theme
        },
        keymap: {
          layer(input: () => { mode?: string; commands?: { id?: string }[] }) {
            input().commands?.forEach((command) => {
              if (command.id) commands.set(command.id, command as never)
            })
            Keymap.createLayer(input as never)
          },
          dispatch: keymap.dispatch,
          shortcuts: shortcuts.list,
          mode: keymap.mode,
        },
        ui: {
          dialog: createDialogApi(dialog, (render) => render()),
          router: {
            register(page: { name: string; render: () => unknown }) {
              if (page.name === "diff") renderDiff = page.render
              return () => {}
            },
            navigate(destination: Route) {
              setCurrent(
                destination.type === "plugin" && !("id" in destination)
                  ? ({ ...destination, id: "opencode.diffs" } as Route)
                  : destination,
              )
            },
            current,
          },
          slot(claim: { render: (props: unknown) => unknown }) {
            renderCommands = claim.render
            return () => {}
          },
        },
      } as never
      void diffViewerPlugin.setup(pluginContext)
      const commandView = renderCommands?.({})
      return (
        <>
          {commandView}
          <Show when={current().type === "plugin" ? current() : undefined} keyed>
            {(route) => renderDiff?.(route.type === "plugin" ? { data: (route as { data?: unknown }).data } : undefined)}
          </Show>
        </>
      )
    }

    return (
      <TestTuiContexts paths={{ state }}>
        <TuiAppProvider value={{ name: "test", version: "test", channel: "test" }}>
          <StorageProvider>
            <ConfigProvider config={config} service={{ get: async () => stored.info, update: async () => stored.info }}>
              <Keymap.Provider>
                <ToastProvider>
                  <ThemeProvider mode="dark" source={emptyThemeSource}>
                    <DialogProvider>
                      <Content />
                    </DialogProvider>
                  </ThemeProvider>
                </ToastProvider>
              </Keymap.Provider>
            </ConfigProvider>
          </StorageProvider>
        </TuiAppProvider>
      </TestTuiContexts>
    )
  }

  const app = await testRender(() => <Harness />, { width: variant.width, height: 24, kittyKeyboard: true })
  await app.waitFor(() => commands.has("diff.open"))
  commands.get("diff.open")!.run()
  await app.waitFor(() => commands.has("diff.close"))
  await app.flush()
  await app.waitForFrame((frame) => !frame.includes("Loading diff") && frame.includes("src/file.txt"))
  await app.renderOnce()
  return app
}

export const scenarios: Scenario[] = [
  {
    id: "diff-viewer-split",
    source: "test/cli/tui/diff-viewer.test.tsx",
    description: "Diff viewer split view rendering three hunks of a modified file (gutter +/- rows)",
    componentClass: "diff-viewer",
    tier: "tier-b",
    variants: [
      { width: 80, height: 24 },
      { width: 160, height: 24 },
    ],
    mount: mountDiffViewer,
  },
]
