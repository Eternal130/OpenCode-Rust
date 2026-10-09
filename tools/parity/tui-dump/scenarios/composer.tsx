/**
 * Composer state goldens (gate-8 finding 2 / gate-9 addition c).
 *
 * Mounts the real session-route `Prompt` component inside the app's own
 * provider stack (mirroring `src/app.tsx` order) so the composer is driven
 * exactly like the session route, while keeping every state deterministic:
 *
 * - placeholder-empty   — single-entry placeholder lists (the app's Home list
 *                         is picked at random; one entry pins index 0)
 * - typed-multiline     — `PromptRef.set()` with a three-line draft
 * - cjk-input           — `PromptRef.set()` with mixed Japanese/ASCII text
 * - attachment-chip     — `PromptRef.set()` with a pasted-image attachment;
 *                         `restoreExtmarksFromPrompt` recreates the `[Image 1]`
 *                         chip extmark in the textarea
 * - autocomplete-open   — `reset()` then typing `/re` opens the slash-command
 *                         autocomplete: filtered list (`/redo`, `/rename`) with
 *                         the selected row highlighted.
 *
 * `ref.set()` is used instead of key events wherever possible — it is the
 * reference's own draft-restore path (draft stash / queued-prompt undo) and
 * avoids input-event timing; only the autocomplete state needs real keys.
 */
/** @jsxImportSource @opentui/solid */
import { testRender } from "@opentui/solid"
import { onMount } from "solid-js"
import { mkdir } from "node:fs/promises"
import path from "node:path"
import type { ModelInfo } from "@opencode/client"
import { ArgsProvider } from "/tmp/opencode/ref/packages/tui/src/context/args"
import { ClientProvider } from "/tmp/opencode/ref/packages/tui/src/context/client"
import { ConfigProvider } from "/tmp/opencode/ref/packages/tui/src/config/index.tsx"
import { DataProvider, useData } from "/tmp/opencode/ref/packages/tui/src/context/data"
import { DialogProvider } from "/tmp/opencode/ref/packages/tui/src/ui/dialog"
import { EditorContextProvider } from "/tmp/opencode/ref/packages/tui/src/context/editor"
import { EpilogueProvider } from "/tmp/opencode/ref/packages/tui/src/context/epilogue"
import { ExitProvider } from "/tmp/opencode/ref/packages/tui/src/context/exit"
import { FrecencyProvider } from "/tmp/opencode/ref/packages/tui/src/prompt/frecency"
import { Keymap } from "/tmp/opencode/ref/packages/tui/src/context/keymap"
import { LocalProvider } from "/tmp/opencode/ref/packages/tui/src/context/local"
import { LocationProvider } from "/tmp/opencode/ref/packages/tui/src/context/location"
import { PanelProvider } from "/tmp/opencode/ref/packages/tui/src/context/panel"
import { AttentionProvider } from "/tmp/opencode/ref/packages/tui/src/context/attention"
import { PermissionProvider } from "/tmp/opencode/ref/packages/tui/src/context/permission"
import { PluginProvider } from "/tmp/opencode/ref/packages/tui/src/plugin/context"
import { PromptHistoryProvider } from "/tmp/opencode/ref/packages/tui/src/prompt/history"
import { PromptRefProvider } from "/tmp/opencode/ref/packages/tui/src/context/prompt"
import { PromptStashProvider } from "/tmp/opencode/ref/packages/tui/src/prompt/stash"
import { RouteProvider } from "/tmp/opencode/ref/packages/tui/src/context/route"
import { SessionTabsProvider } from "/tmp/opencode/ref/packages/tui/src/context/session-tabs"
import { SessionTerminalsProvider } from "/tmp/opencode/ref/packages/tui/src/context/session-terminals"
import { StorageProvider } from "/tmp/opencode/ref/packages/tui/src/context/storage"
import { ThemeProvider } from "/tmp/opencode/ref/packages/tui/src/context/theme"
import { TuiAppProvider, TuiLifecycleProvider } from "/tmp/opencode/ref/packages/tui/src/context/runtime"
import { ToastProvider } from "/tmp/opencode/ref/packages/tui/src/ui/toast"
import { UpdateNotificationProvider } from "/tmp/opencode/ref/packages/tui/src/context/update-notification"
import { Prompt, type PromptRef } from "/tmp/opencode/ref/packages/tui/src/component/prompt"
import { emptyThemeSource, tmpdir } from "/tmp/opencode/ref/packages/tui/test/fixture/fixture"
import { TestTuiContexts } from "/tmp/opencode/ref/packages/tui/test/fixture/tui-environment"
import { createTuiResolvedConfig } from "/tmp/opencode/ref/packages/tui/test/fixture/tui-runtime"
import { createApi, createEventStream, createFetch, json } from "/tmp/opencode/ref/packages/tui/test/fixture/tui-client"
import type { PromptInfo } from "/tmp/opencode/ref/packages/tui/src/prompt/history"
import type { Scenario, ScenarioVariant } from "../scenario"

const directory = "/tmp/opencode/packages/tui"

function model(id: string): ModelInfo {
  return {
    id,
    modelID: id,
    providerID: "opencode",
    name: id === "gpt-5" ? "GPT-5" : id,
    status: "active",
    enabled: true,
    capabilities: { input: ["text"], output: ["text"], tools: true },
    cost: [],
    limit: { context: 10000, output: 1000 },
    time: { released: 0 },
    variants: [],
  }
}

const agent = {
  id: "build",
  name: "Build",
  model: { providerID: "opencode", modelID: "gpt-5" },
  mode: "primary",
  hidden: false,
  permissions: [],
  request: { settings: {}, headers: {}, body: {} },
}

/** One tiny 1×1 PNG so the attachment chip mirrors a real pasted image. */
const ATTACHMENT_PNG =
  "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=="

const CHIP_TEXT = "Compare [Image 1] with the current design"

const drafts: Record<string, PromptInfo> = {
  typed: {
    text: "Refactor the parser module\nkeep the public API stable\nadd regression tests",
    files: [],
    agents: [],
    pasted: [],
  },
  cjk: {
    text: "検証：日本語プロンプトの入力（English まじり）",
    files: [],
    agents: [],
    pasted: [],
  },
  chip: {
    text: CHIP_TEXT,
    files: [
      {
        uri: ATTACHMENT_PNG,
        name: "screenshot.png",
        mention: { start: CHIP_TEXT.indexOf("[Image 1]"), end: CHIP_TEXT.indexOf("[Image 1]") + "[Image 1]".length, text: "[Image 1]" },
      },
    ],
    agents: [],
    pasted: [],
  },
}

type Drive = (ref: PromptRef, app: Awaited<ReturnType<typeof testRender>>, data: ReturnType<typeof useData>) => Promise<void>

async function mountPrompt(sessionID: string, drive: Drive, variant: ScenarioVariant) {
  const root = (await tmpdir()).path
  const state = path.join(root, "state")
  await mkdir(state, { recursive: true })
  await Bun.write(path.join(state, "model.json"), "{}")
  const events = createEventStream()
  const fetch = createFetch((url) => {
    const location = { directory, project: { id: "proj_test", directory: "/tmp/opencode" } }
    if (url.pathname === "/api/agent") return json({ location, data: [agent] })
    if (url.pathname === "/api/model") return json({ location, data: [model("gpt-5"), model("gpt-mini")] })
    if (url.pathname === `/api/session/${sessionID}`)
      return json({
        data: {
          id: sessionID,
          title: "Composer fixture",
          projectID: "proj_test",
          location: { directory },
          cost: 0,
          tokens: { input: 0, output: 0, reasoning: 0, cache: { read: 0, write: 0 } },
          time: { created: 0, updated: 0 },
        },
      })
    return undefined
  }, events)

  let ref: PromptRef | undefined
  let data: ReturnType<typeof useData> | undefined
  function Harness() {
    Keymap.createLayer(() => ({
      mode: "global",
      commands: [
        { id: "session.rename", title: "Rename session", group: "Session", slash: { name: "rename" }, run() {} },
        { id: "session.redo", title: "Redo", group: "Session", slash: { name: "redo" }, run() {} },
        { id: "session.compact", title: "Compact session", group: "Session", slash: { name: "compact" }, run() {} },
      ],
    }))
    return (
      <box flexDirection="column" paddingTop={2}>
        <Prompt
          ref={(r) => (ref = r)}
          sessionID={sessionID}
          placeholders={{ normal: ["Fix a TODO in the codebase"], shell: ["ls -la"] }}
        />
      </box>
    )
  }
  function Probe() {
    data = useData()
    onMount(() => void data!.session.sync(sessionID))
    return null
  }

  const app = await testRender(
    () => (
      <TestTuiContexts directory={directory} paths={{ home: root, state, worktree: root }}>
        <TuiAppProvider value={{ name: "test", version: "test", channel: "test" }}>
          <ExitProvider exit={() => {}}>
            <EpilogueProvider set={() => {}}>
              <StorageProvider>
                <TuiLifecycleProvider value={{ add: () => () => {} }}>
                  <ArgsProvider>
                    <ConfigProvider config={createTuiResolvedConfig()}>
                      <Keymap.Provider>
                        <ToastProvider>
                          <RouteProvider initialRoute={{ type: "session", sessionID }}>
                            <ClientProvider api={createApi(fetch.fetch)}>
                              <PermissionProvider>
                                <DataProvider directory={directory}>
                                  <LocationProvider>
                                    <SessionTabsProvider>
                                      <SessionTerminalsProvider>
                                        <ThemeProvider mode="dark" source={emptyThemeSource}>
                                          <LocalProvider>
                                            <PromptStashProvider>
                                              <DialogProvider>
                                                <FrecencyProvider>
                                                  <PromptHistoryProvider>
                                                    <PromptRefProvider>
                                                      <EditorContextProvider>
                                                        <AttentionProvider>
                                                          <UpdateNotificationProvider updater={undefined}>
                                                            <PanelProvider>
                                                              <PluginProvider packages={{ prepare: async () => ({ directory: "" }) }} directories={[]}>
                                                                <Probe />
                                                                <Harness />
                                                              </PluginProvider>
                                                            </PanelProvider>
                                                          </UpdateNotificationProvider>
                                                        </AttentionProvider>
                                                      </EditorContextProvider>
                                                    </PromptRefProvider>
                                                  </PromptHistoryProvider>
                                                </FrecencyProvider>
                                              </DialogProvider>
                                            </PromptStashProvider>
                                          </LocalProvider>
                                        </ThemeProvider>
                                      </SessionTerminalsProvider>
                                    </SessionTabsProvider>
                                  </LocationProvider>
                                </DataProvider>
                              </PermissionProvider>
                            </ClientProvider>
                          </RouteProvider>
                        </ToastProvider>
                      </Keymap.Provider>
                    </ConfigProvider>
                  </ArgsProvider>
                </TuiLifecycleProvider>
              </StorageProvider>
            </EpilogueProvider>
          </ExitProvider>
        </TuiAppProvider>
      </TestTuiContexts>
    ),
    { ...variant, kittyKeyboard: true },
  )
  app.renderer.start()
  await app.waitFor(() => ref !== undefined && data !== undefined)
  await app.waitFor(() => data!.session.get(sessionID) !== undefined, { maxPasses: 200 })
  await drive(ref!, app, data!)
  await app.waitForVisualIdle()
  await app.renderOnce()
  return app
}

function state(id: string, description: string, drive: Drive): Scenario {
  return {
    id: `composer-${id}`,
    source: "packages/tui/src/component/prompt (state corpus; drive via PromptRef.set)",
    description,
    componentClass: "composer",
    tier: "tier-a",
    variants: [
      { width: 80, height: 20 },
      { width: 40, height: 20 },
    ],
    mount: (variant) => mountPrompt(`ses_composer_${id.replace(/-/g, "_")}`, drive, variant),
  }
}

export const scenarios: Scenario[] = [
  state("placeholder-empty", "Empty composer with a single-entry placeholder list (Ask anything… prompt)", async () => {}),
  state("typed-multiline", "Composer holding a three-line typed draft (multi-row input, caret at end)", async (ref) => {
    ref.set(drafts.typed)
  }),
  state("cjk-input", "Composer holding a mixed Japanese/ASCII draft (double-width glyphs in the input)", async (ref) => {
    ref.set(drafts.cjk)
  }),
  state(
    "attachment-chip",
    "Composer with a pasted-image attachment: [Image 1] chip rendered as a file extmark inside the input",
    async (ref) => {
      ref.set(drafts.chip)
    },
  ),
  state(
    "autocomplete-open",
    "Slash-command autocomplete open on '/re': filtered list (/redo, /rename) with the selected row highlighted",
    async (ref, app) => {
      // reset() first: the module-level draft stash restores the previous
      // mount's draft (same sessionID), which would otherwise shift the text.
      ref.reset()
      await app.renderOnce()
      await app.mockInput.typeText("/re")
      await app.waitForFrame((frame) => frame.includes("Rename session"), { maxPasses: 100 })
    },
  ),
]
