/**
 * Frozen spinner golden — gate-8 approval tier C1 (ManualClock frozen frame).
 *
 * Renders the tool-running row exactly as `GroupContent` paints an in-flight
 * exploration group in `routes/session/group-view.tsx`: `InlineToolRow` with
 * `spinner` + `complete={false}` + the pending label, muted theme color.
 * The renderer runs on a `ManualClock` pinned at t=0, so the `<spinner>`
 * renderable shows frame 0 of SPINNER_FRAMES ("⠋") and never advances — one
 * tick, no animation loop. Byte-stable across mounts because the clock, not
 * the wall, drives the frame index.
 */
/** @jsxImportSource @opentui/solid */
import { testRender } from "@opentui/solid"
import { ManualClock } from "@opentui/core/testing"
import { ConfigProvider } from "/tmp/opencode/ref/packages/tui/src/config/index.tsx"
import { ThemeProvider, useTheme } from "/tmp/opencode/ref/packages/tui/src/context/theme"
import { InlineToolRow } from "/tmp/opencode/ref/packages/tui/src/routes/session/index"
import { emptyThemeSource } from "/tmp/opencode/ref/packages/tui/test/fixture/fixture"
import { createTuiResolvedConfig } from "/tmp/opencode/ref/packages/tui/test/fixture/tui-runtime"
import type { Scenario, ScenarioVariant } from "../scenario"

function RunningRow() {
  const theme = useTheme()
  return (
    <box width="100%" paddingTop={1}>
      <InlineToolRow
        icon="✱"
        color={theme.text.muted}
        complete={false}
        pending="Exploring: 2 searches, 1 read"
        spinner={true}
      >
        Exploring: 2 searches, 1 read
      </InlineToolRow>
    </box>
  )
}

async function mount(variant: ScenarioVariant) {
  const config = createTuiResolvedConfig()
  const clock = new ManualClock()
  const app = await testRender(
    () => (
      <ConfigProvider config={config}>
        <ThemeProvider mode="dark" source={emptyThemeSource}>
          <RunningRow />
        </ThemeProvider>
      </ConfigProvider>
    ),
    { ...variant, clock, kittyKeyboard: true },
  )
  await app.renderOnce()
  await app.renderOnce()
  return app
}

export const scenarios: Scenario[] = [
  {
    id: "spinner-frozen",
    source: "packages/tui/src/routes/session/group-view.tsx (running InlineToolRow, ManualClock)",
    description: "Tool-running row frozen at t=0: spinner glyph ⠋ beside its pending label, clock never advances",
    componentClass: "spinner",
    tier: "tier-c1",
    variants: [
      { width: 80, height: 6 },
      { width: 40, height: 6 },
    ],
    mount,
  },
]
