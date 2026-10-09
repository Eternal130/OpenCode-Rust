/** @jsxImportSource @opentui/solid */
import { testRender } from "@opentui/solid"
import { mkdir } from "node:fs/promises"
import path from "node:path"
import { createSignal, onCleanup, onMount, type JSX } from "solid-js"
import { addDefaultParsers } from "@opentui/core"
import parsers from "/tmp/opencode/ref/packages/tui/src/parsers-config"
import { ConfigProvider, resolve } from "/tmp/opencode/ref/packages/tui/src/config/index.tsx"
import { CommandPaletteDialog } from "/tmp/opencode/ref/packages/tui/src/component/command-palette"
import { SessionGroupView } from "/tmp/opencode/ref/packages/tui/src/routes/session/group-view"
import { createTimelineAnchors, groupID } from "/tmp/opencode/ref/packages/tui/src/routes/session/anchors"
import { context } from "/tmp/opencode/ref/packages/tui/src/routes/session/render-context"
import type { SessionGroup, SessionEntry } from "/tmp/opencode/ref/packages/tui/src/routes/session/grouping/session"
import { DataProvider } from "/tmp/opencode/ref/packages/tui/src/context/data"
import { ClientProvider } from "/tmp/opencode/ref/packages/tui/src/context/client"
import { ThemeProvider } from "/tmp/opencode/ref/packages/tui/src/context/theme"
import { Keymap } from "/tmp/opencode/ref/packages/tui/src/context/keymap"
import { DialogProvider, useDialog } from "/tmp/opencode/ref/packages/tui/src/ui/dialog.tsx"
import { DialogSelect } from "/tmp/opencode/ref/packages/tui/src/ui/dialog-select"
import { ToastProvider } from "/tmp/opencode/ref/packages/tui/src/ui/toast"
import { FormPrompt } from "/tmp/opencode/ref/packages/tui/src/routes/session/form"
import type { FormWithLocation } from "/tmp/opencode/ref/packages/tui/src/context/data"
import { emptyThemeSource, tmpdir } from "/tmp/opencode/ref/packages/tui/test/fixture/fixture"
import { TestTuiContexts } from "/tmp/opencode/ref/packages/tui/test/fixture/tui-environment"
import { createTuiResolvedConfig } from "/tmp/opencode/ref/packages/tui/test/fixture/tui-runtime"
import { createApi, createEventStream, createFetch, json } from "/tmp/opencode/ref/packages/tui/test/fixture/tui-client"
import type { Scenario, ScenarioVariant } from "../scenario"

async function mountFormPrompt(variant: ScenarioVariant) {
  const root = (await tmpdir()).path
  const state = path.join(root, "state")
  await mkdir(state, { recursive: true })
  const events = createEventStream()
  const form = {
    id: "frm_test",
    sessionID: "ses_test",
    title: "Authorization required",
    fields: [
      {
        key: "authorization",
        type: "external",
        url: "https://example.com/authorize",
        title: "Authorize access",
      },
    ],
  } satisfies FormWithLocation
  const transport = createFetch((url, request) => {
    if (url.pathname === "/api/session/ses_test/form" && request.method === "GET")
      return json({ data: [form] })
    if (url.pathname === "/api/session/ses_test/form/frm_test/reply") return new Response(null, { status: 204 })
    if (url.pathname === "/api/session/ses_test/form/frm_test" && request.method === "DELETE")
      return new Response(null, { status: 204 })
    return undefined
  }, events)
  const config = createTuiResolvedConfig()

  function Harness() {
    return (
      <TestTuiContexts directory={root} paths={{ home: root, state, worktree: root }}>
        <ConfigProvider config={config}>
          <Keymap.Provider>
            <ClientProvider api={createApi(transport.fetch)}>
              <DataProvider directory={root}>
                <ThemeProvider mode="dark" source={emptyThemeSource}>
                  <ToastProvider>
                    <FormPrompt form={form} />
                  </ToastProvider>
                </ThemeProvider>
              </DataProvider>
            </ClientProvider>
          </Keymap.Provider>
        </ConfigProvider>
      </TestTuiContexts>
    )
  }

  const app = await testRender(() => <Harness />, { ...variant, kittyKeyboard: true })
  app.renderer.start()
  await app.waitForFrame((frame) => frame.includes("Authorization required"))
  await app.renderOnce()
  return app
}

async function mountCommandPalette(variant: ScenarioVariant) {
  const config = resolve({}, { terminalSuspend: true })
  function Fixture() {
    const dialog = useDialog()
    Keymap.createLayer(() => ({
      mode: "global",
      commands: [
        { id: "session.new", title: "New session", group: "Session", palette: true, run() {} },
        { id: "model.list", title: "Switch model", group: "Agent", palette: true, run() {} },
      ],
    }))
    onMount(() => dialog.replace(() => <CommandPaletteDialog />))
    return null
  }

  const app = await testRender(
    () => (
      <TestTuiContexts>
        <ConfigProvider config={config}>
          <Keymap.Provider>
            <ThemeProvider mode="dark" source={emptyThemeSource}>
              <ToastProvider>
                <DialogProvider>
                  <Fixture />
                </DialogProvider>
              </ToastProvider>
            </ThemeProvider>
          </Keymap.Provider>
        </ConfigProvider>
      </TestTuiContexts>
    ),
    { ...variant, kittyKeyboard: true },
  )
  app.renderer.start()
  await app.waitForFrame((frame) => frame.includes("New session"))
  await app.renderOnce()
  return app
}

async function mountDialogSelect(variant: ScenarioVariant) {
  const root = (await tmpdir()).path
  const state = path.join(root, "state")
  await mkdir(state, { recursive: true })
  const config = createTuiResolvedConfig()

  function Harness() {
    function Select() {
      onCleanup(Keymap.use().mode.push("modal"))
      return (
        <DialogSelect
          title="Items"
          options={[
            { title: "Alpha", value: "a", description: "First item" },
            { title: "Beta", value: "b", description: "Second item" },
            { title: "Gamma", value: "c", description: "Third item" },
          ]}
          current={"b" as never}
          actions={[
            { command: "dialog.move_session.delete", title: "delete", onTrigger: () => {} },
            { command: "dialog.move_session.new", title: "new", selection: "none", onTrigger: () => {} },
          ]}
        />
      )
    }
    return (
      <TestTuiContexts directory={root} paths={{ home: root, state, worktree: root }}>
        <ConfigProvider config={config}>
          <Keymap.Provider>
            <ThemeProvider mode="dark" source={emptyThemeSource}>
              <ToastProvider>
                <DialogProvider>
                  <Select />
                </DialogProvider>
              </ToastProvider>
            </ThemeProvider>
          </Keymap.Provider>
        </ConfigProvider>
      </TestTuiContexts>
    )
  }

  const app = await testRender(() => <Harness />, { ...variant, kittyKeyboard: true })
  app.renderer.start()
  await app.waitForFrame((frame) => frame.includes("Items"))
  await app.renderOnce()
  return app
}

let parsersReady = false
async function mountGroupView(variant: ScenarioVariant) {
  if (!parsersReady) {
    addDefaultParsers(parsers.parsers)
    parsersReady = true
  }
  const anchors = createTimelineAnchors()
  const messages = ["AGENTS.md", "packages/tui/AGENTS.md"].map((file) => ({
    type: "synthetic" as const,
    id: file,
    text: "Instructions",
    description: `Loaded ${file}`,
    metadata: { instruction: { paths: [file] } },
    time: { created: 1 },
  }))
  const row: SessionGroup = {
    type: "group",
    kind: "instructions",
    size: messages.length,
    completed: true,
    children: messages.map((message) => ({
      type: "entry" as const,
      size: 1,
      entry: { type: "message" as const, messageID: message.id },
    })),
  }
  const config = createTuiResolvedConfig({ animations: false })
  function entry(node: SessionEntry): JSX.Element {
    return <text>{node.type === "message" ? `Loaded ${node.messageID}` : ""}</text>
  }

  const app = await testRender(
    () => (
      <TestTuiContexts>
        <ConfigProvider config={config}>
          <ThemeProvider mode="dark" source={emptyThemeSource}>
            <context.Provider
              value={{
                width: variant.width,
                terminal: { width: variant.width, height: 24 },
                sessionID: "fixture",
                anchors,
                groupExpanded: () => false,
                setGroupExpanded: () => {},
                thinkingMode: () => "hide",
                markdownMode: () => "rendered",
                groupExploration: () => true,
                legacyTurns: () => false,
                diffWrapMode: () => "word",
                models: () => [],
                messageIndex: () => undefined,
                config,
                mutatePending: async () => true,
                pendingDelivery: () => undefined,
              }}
            >
              <box paddingTop={2}>
                <SessionGroupView row={row} message={(id: string) => messages.find((m) => m.id === id)} images={() => <text>Image previews</text>} entry={entry} />
              </box>
            </context.Provider>
          </ThemeProvider>
        </ConfigProvider>
      </TestTuiContexts>
    ),
    variant,
  )
  app.renderer.start()
  await app.waitForFrame((frame) => frame.includes("Instructions: 2 files"))
  await app.renderOnce()
  return app
}

export const scenarios: Scenario[] = [
  {
    id: "form-authorization",
    source: "test/cli/tui/form.test.tsx",
    description: "Session FormPrompt: external authorization form (acknowledgement required)",
    componentClass: "prompt-composer",
    tier: "tier-a",
    variants: [
      { width: 80, height: 20 },
      { width: 160, height: 20 },
    ],
    mount: mountFormPrompt,
  },
  {
    id: "command-palette",
    source: "test/cli/tui/command-palette.test.tsx",
    description: "CommandPaletteDialog listing two registered commands (New session / Switch model)",
    componentClass: "dialog",
    tier: "tier-a",
    variants: [
      { width: 80, height: 24 },
      { width: 160, height: 24 },
    ],
    mount: mountCommandPalette,
  },
  {
    id: "dialog-select",
    source: "test/cli/tui/dialog-select.test.tsx",
    description: "DialogSelect fuzzy picker with three items, Beta selected, two row actions",
    componentClass: "dialog",
    tier: "tier-a",
    variants: [
      { width: 80, height: 20 },
      { width: 160, height: 20 },
    ],
    mount: mountDialogSelect,
  },
  {
    id: "group-view-instructions",
    source: "test/cli/tui/group-view.test.tsx",
    description: "SessionGroupView collapsed instructions group ('Instructions: 2 files' summary row)",
    componentClass: "session-group",
    tier: "tier-a",
    variants: [
      { width: 80, height: 24 },
      { width: 160, height: 24 },
    ],
    mount: mountGroupView,
  },
]
