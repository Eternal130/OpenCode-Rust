/** @jsxImportSource @opentui/solid */
import { testRender } from "@opentui/solid"
import { batch, createSignal } from "solid-js"
import { ConfigProvider } from "/tmp/opencode/ref/packages/tui/src/config"
import {
  EMPTY_SESSION_TAB_STATUS,
  SessionTabs,
  type SessionTabsController,
  type SessionTabsStatus,
} from "/tmp/opencode/ref/packages/tui/src/component/session-tabs.tsx"
import { ClientProvider } from "/tmp/opencode/ref/packages/tui/src/context/client"
import { DataProvider } from "/tmp/opencode/ref/packages/tui/src/context/data"
import { LocationProvider } from "/tmp/opencode/ref/packages/tui/src/context/location"
import { Keymap } from "/tmp/opencode/ref/packages/tui/src/context/keymap"
import { RouteProvider } from "/tmp/opencode/ref/packages/tui/src/context/route"
import { TuiAppProvider } from "/tmp/opencode/ref/packages/tui/src/context/runtime"
import { SessionTabsProvider } from "/tmp/opencode/ref/packages/tui/src/context/session-tabs"
import { StorageProvider } from "/tmp/opencode/ref/packages/tui/src/context/storage"
import { ThemeProvider } from "/tmp/opencode/ref/packages/tui/src/context/theme"
import { DialogProvider } from "/tmp/opencode/ref/packages/tui/src/ui/dialog.tsx"
import { ToastProvider } from "/tmp/opencode/ref/packages/tui/src/ui/toast"
import { emptyThemeSource, tmpdir } from "/tmp/opencode/ref/packages/tui/test/fixture/fixture"
import { createApi, createEventStream, createFetch } from "/tmp/opencode/ref/packages/tui/test/fixture/tui-client"
import { TestTuiContexts } from "/tmp/opencode/ref/packages/tui/test/fixture/tui-environment"
import { createTuiResolvedConfig } from "/tmp/opencode/ref/packages/tui/test/fixture/tui-runtime"
import { cleanup, type Scenario, type ScenarioVariant } from "../scenario"

async function mountTabs(orientation: "horizontal" | "vertical", variant: ScenarioVariant) {
  const temporary = await tmpdir()
  const [status] = createSignal<SessionTabsStatus>({ ...EMPTY_SESSION_TAB_STATUS, unread: "activity" })
  const [active] = createSignal("second")
  const [newTab] = createSignal(false)
  const settings = { tabs: { mode: "on" } }
  const controller = {
    tabs: () => [
      { sessionID: "first", title: "First" },
      { sessionID: "second", title: "Second" },
    ],
    current: active,
    newTab,
    select() {},
    close() {},
    move() {},
    add() {},
    recentlyClosed: () => [],
    reopen() {},
    detail: () => "project",
    status: (sessionID: string) => (sessionID === "first" ? status() : EMPTY_SESSION_TAB_STATUS),
  } satisfies SessionTabsController

  function Harness() {
    return (
      <TestTuiContexts paths={{ state: temporary.path }} clipboard={{ read: async () => undefined, write: async () => {} }}>
        <TuiAppProvider value={{ name: "test", version: "test", channel: "test" }}>
          <StorageProvider>
            <ConfigProvider
              config={createTuiResolvedConfig(settings)}
              service={{
                get: async () => settings,
                update: async (update) => {
                  update(settings)
                  return settings
                },
              }}
            >
              <RouteProvider initialRoute={{ type: "home" }}>
                <ClientProvider api={createApi(createFetch(undefined, createEventStream()).fetch)}>
                  <DataProvider directory={temporary.path}>
                    <LocationProvider>
                      <SessionTabsProvider>
                        <ThemeProvider mode="dark" source={emptyThemeSource}>
                          <Keymap.Provider>
                            <ToastProvider>
                              <DialogProvider>
                                <box width="100%" height="100%">
                                  <SessionTabs controller={controller} orientation={orientation} animations={false} />
                                </box>
                              </DialogProvider>
                            </ToastProvider>
                          </Keymap.Provider>
                        </ThemeProvider>
                      </SessionTabsProvider>
                    </LocationProvider>
                  </DataProvider>
                </ClientProvider>
              </RouteProvider>
            </ConfigProvider>
          </StorageProvider>
        </TuiAppProvider>
      </TestTuiContexts>
    )
  }

  const app = await testRender(() => <Harness />, variant)
  app.renderer.start()
  await app.waitForFrame((frame) => frame.includes("First") && frame.includes("Second"))
  await app.renderOnce()
  return app
}

export const scenarios: Scenario[] = (
  [
    ["horizontal", 10, "session-tabs-horizontal"],
    ["vertical", 16, "session-tabs-vertical"],
  ] as const
).map(([orientation, height, id]) => ({
  id,
  source: "test/component/session-tabs-status.test.tsx",
  description: `SessionTabs ${orientation} bar/rail with two tabs; first tab carries an unread-activity indicator`,
  componentClass: "session-tabs",
  tier: "tier-a",
  variants: [
    { width: 80, height },
    { width: 160, height },
  ],
  mount: (variant: ScenarioVariant) => mountTabs(orientation, variant),
}))
