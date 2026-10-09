/** @jsxImportSource @opentui/solid */
import { testRender } from "@opentui/solid"
import { createSignal } from "solid-js"
import type { FormInfo } from "@opencode/client/promise"
import { Keymap } from "/tmp/opencode/ref/packages/tui/src/context/keymap"
import {
  RUN_COMMAND_PANEL_ROWS,
  RunAgentSelectBody,
  RunCommandMenuBody,
  RunModelSelectBody,
  RunSettingsBody,
  RunVariantSelectBody,
} from "/tmp/opencode/ref/packages/tui/src/mini/footer.command"
import { RunFooterView } from "/tmp/opencode/ref/packages/tui/src/mini/footer.view"
import { RejectField } from "/tmp/opencode/ref/packages/tui/src/mini/footer.permission"
import { RUN_THEME_FALLBACK, type RunTheme } from "/tmp/opencode/ref/packages/tui/src/mini/theme"
import { resolveMiniSettings } from "/tmp/opencode/ref/packages/tui/src/mini/runtime.boot"
import type {
  FooterState,
  FooterSubagentState,
  FooterView,
  RunAgent,
  RunCommand,
  RunInput,
  RunProvider,
  RunPrompt,
  RunTuiConfig,
} from "/tmp/opencode/ref/packages/tui/src/mini/types"
import { createTuiResolvedConfig } from "/tmp/opencode/ref/packages/tui/test/fixture/tui-runtime"
import type { Scenario, ScenarioVariant } from "../scenario"

const tuiConfig: RunTuiConfig = createTuiResolvedConfig()

function footerState(input: Partial<FooterState> = {}) {
  return createSignal<FooterState>({
    phase: "idle",
    status: "",
    notice: "",
    model: "gpt-5",
    usage: undefined,
    first: false,
    interrupt: 0,
    exit: 0,
    ...input,
  })
}

interface FooterInput {
  view?: FooterView
  height?: number
  state?: Partial<FooterState>
}

async function renderFooter(input: FooterInput, variant: ScenarioVariant) {
  const [view] = createSignal<FooterView>(input.view ?? { type: "prompt" })
  const [subagents] = createSignal<FooterSubagentState>({ tabs: [], details: {}, permissions: [], forms: [] })
  const [state] = footerState(input.state)
  const [queuedPrompts] = createSignal([])
  const config = { ...tuiConfig, animations: false }
  const [miniSettings] = createSignal(resolveMiniSettings())
  const height = input.height ?? 12

  function Harness() {
    return (
      <Keymap.Provider config={config}>
        <RunFooterView
          directory={() => "/tmp"}
          findFiles={async () => []}
          agents={() => []}
          references={() => []}
          commands={() => []}
          providers={() => undefined}
          currentAgent={() => "Build"}
          currentAgentID={() => "build"}
          currentModel={() => undefined}
          variants={() => []}
          currentVariant={() => undefined}
          state={state}
          view={view}
          subagent={subagents}
          queuedPrompts={queuedPrompts}
          theme={() => RUN_THEME_FALLBACK}
          tuiConfig={config}
          mono={false}
          miniSettings={miniSettings}
          onSubmit={() => true}
          clipboard={{ async read() { return undefined }, async write() {} }}
          history={() => []}
          onPermissionReply={() => {}}
          onFormReply={() => {}}
          onFormCancel={() => {}}
          onCycle={() => {}}
          onInterrupt={() => false}
          onQueuedPromptAction={() => {}}
          onEditorOpen={async () => undefined}
          onInputClear={() => {}}
          onExit={() => {}}
          onAgentSelect={() => {}}
          onModelSelect={() => {}}
          onVariantSelect={() => {}}
          onRows={() => {}}
          onLayout={() => {}}
          onStatus={() => {}}
          onMiniSettingChange={() => {}}
        />
      </Keymap.Provider>
    )
  }

  const app = await testRender(
    () => (
      <box width="100%" height={height}>
        <Harness />
      </box>
    ),
    { width: variant.width, height: variant.height, kittyKeyboard: true },
  )
  await app.flush()
  await app.renderOnce()
  return app
}

function command(input: { name: string; description: string; source?: "command" | "mcp" | "skill" }): RunCommand {
  return { name: input.name, description: input.description, source: input.source }
}

function model(input: { id: string; name: string; status?: "active" | "deprecated"; cost?: number }) {
  return {
    name: input.name,
    cost: { input: input.cost ?? 1 },
    status: input.status ?? "active",
  }
}

function provider(): RunProvider {
  return {
    id: "opencode",
    name: "opencode",
    models: {
      "gpt-5": model({ id: "gpt-5", name: "GPT-5" }),
      "gpt-free": model({ id: "gpt-free", name: "GPT Free", cost: 0 }),
      old: model({ id: "old", name: "Old Model", status: "deprecated" }),
    },
  }
}

export const scenarios: Scenario[] = [
  {
    id: "footer-prompt-idle",
    source: "test/mini/footer.view.test.tsx",
    description: "RunFooterView default prompt composer (idle phase, fallback dark theme)",
    componentClass: "mini-footer",
    variants: [
      { width: 80, height: 12 },
      { width: 160, height: 12 },
    ],
    mount: (variant) => renderFooter({}, variant),
  },
  {
    id: "footer-permission",
    source: "test/mini/footer.view.test.tsx",
    description: "RunFooterView permission view: 'Permission required' with Allow once / Reject actions",
    componentClass: "mini-footer-permission",
    variants: [
      { width: 80, height: 16 },
      { width: 160, height: 16 },
    ],
    mount: (variant) =>
      renderFooter(
        {
          height: 16,
          view: {
            type: "permission",
            request: { id: "per_dump", sessionID: "ses_dump", action: "read", resources: ["src/index.ts"] },
          },
        },
        variant,
      ),
  },
  {
    id: "footer-form",
    source: "test/mini/footer.view.test.tsx",
    description: "RunFooterView multi-field form view (Deployment: Service + Notes)",
    componentClass: "mini-footer-form",
    variants: [
      { width: 80, height: 16 },
      { width: 160, height: 16 },
    ],
    mount: (variant) =>
      renderFooter(
        {
          height: 16,
          view: {
            type: "form",
            request: {
              id: "frm_preempted",
              sessionID: "ses_child",
              title: "Deployment",
              fields: [
                { key: "service", type: "string", title: "Service", required: true },
                { key: "notes", type: "string", title: "Notes", required: true },
              ],
            } as FormInfo,
          },
        },
        variant,
      ),
  },
  {
    id: "permission-reject-field",
    source: "test/mini/footer.view.test.tsx",
    description: "RejectField free-text reason input shown with permission actions",
    componentClass: "mini-footer-permission",
    variants: [
      { width: 80, height: 18 },
      { width: 160, height: 18 },
    ],
    mount: async (variant) => {
      function Harness() {
        return (
          <Keymap.Provider config={tuiConfig}>
            <RejectField
              theme={RUN_THEME_FALLBACK.footer}
              text=""
              disabled={false}
              onChange={() => {}}
              onConfirm={() => {}}
              onCancel={() => {}}
            />
          </Keymap.Provider>
        )
      }
      const app = await testRender(
        () => (
          <box width="100%" height={18}>
            <Harness />
          </box>
        ),
        { ...variant, kittyKeyboard: true },
      )
      await app.renderOnce()
      await app.renderOnce()
      return app
    },
  },
  {
    id: "footer-command-menu",
    source: "test/mini/footer.view.test.tsx",
    description: "RunCommandMenuBody grouped commands panel (Session/Agent/Editor sections + MCP command)",
    componentClass: "mini-footer-panel",
    variants: [
      { width: 80, height: RUN_COMMAND_PANEL_ROWS + 1 },
      { width: 160, height: RUN_COMMAND_PANEL_ROWS + 1 },
    ],
    mount: async (variant) => {
      const [commands] = createSignal<RunCommand[] | undefined>([
        command({ name: "review", description: "Review code" }),
        command({ name: "deploy", description: "Deploy prompt", source: "mcp" }),
      ])
      const [subagents] = createSignal([])
      const [variants] = createSignal(["high", "minimal"])
      const app = await testRender(
        () => (
          <box width="100%" height={RUN_COMMAND_PANEL_ROWS}>
            <RunCommandMenuBody
              theme={() => RUN_THEME_FALLBACK.footer}
              commands={commands}
              subagents={subagents}
              queued={() => []}
              variants={variants}
              variantCycle="ctrl+t"
              onClose={() => {}}
              onAgent={() => {}}
              onModel={() => {}}
              onEditor={() => {}}
              onSubagent={() => {}}
              onQueued={() => {}}
              onVariant={() => {}}
              onVariantCycle={() => {}}
              onStatus={() => {}}
              onSettings={() => {}}
              onCommand={() => {}}
              onNew={() => {}}
              onExit={() => {}}
            />
          </box>
        ),
        { width: variant.width, height: variant.height },
      )
      await app.renderOnce()
      await app.renderOnce()
      return app
    },
  },
  {
    id: "footer-model-select",
    source: "test/mini/footer.view.test.tsx",
    description: "RunModelSelectBody provider/model picker (opencode: GPT-5, GPT Free, deprecated Old Model)",
    componentClass: "mini-footer-panel",
    variants: [
      { width: 80, height: RUN_COMMAND_PANEL_ROWS + 1 },
      { width: 160, height: RUN_COMMAND_PANEL_ROWS + 1 },
    ],
    mount: async (variant) => {
      const [providers] = createSignal<RunProvider[] | undefined>([provider()])
      const [current] = createSignal<RunInput["model"]>({ providerID: "opencode", modelID: "gpt-5" })
      const app = await testRender(
        () => (
          <box width="100%" height={RUN_COMMAND_PANEL_ROWS}>
            <RunModelSelectBody
              theme={() => RUN_THEME_FALLBACK.footer}
              providers={providers}
              current={current}
              onClose={() => {}}
              onSelect={() => {}}
            />
          </box>
        ),
        { width: variant.width, height: variant.height },
      )
      await app.renderOnce()
      await app.renderOnce()
      return app
    },
  },
  {
    id: "footer-agent-select",
    source: "test/mini/footer.view.test.tsx",
    description: "RunAgentSelectBody agent picker (Build/Review/Explore, current=review, hidden agent excluded)",
    componentClass: "mini-footer-panel",
    variants: [
      { width: 80, height: RUN_COMMAND_PANEL_ROWS },
      { width: 160, height: RUN_COMMAND_PANEL_ROWS },
    ],
    mount: async (variant) => {
      const [agents] = createSignal<RunAgent[]>([
        { id: "build", name: "Build", description: "Build software", mode: "all", hidden: false },
        { id: "review", name: "Review", description: "Review changes", mode: "primary", hidden: false },
        { id: "explore", name: "Explore", mode: "subagent", hidden: false },
        { id: "secret", name: "Secret", mode: "all", hidden: true },
      ])
      const [current] = createSignal("review")
      const app = await testRender(
        () => (
          <box width="100%" height={RUN_COMMAND_PANEL_ROWS}>
            <RunAgentSelectBody
              theme={() => RUN_THEME_FALLBACK.footer}
              agents={agents}
              current={current}
              onClose={() => {}}
              onSelect={() => {}}
            />
          </box>
        ),
        { width: variant.width, height: variant.height },
      )
      await app.renderOnce()
      await app.renderOnce()
      return app
    },
  },
  {
    id: "footer-variant-select",
    source: "test/mini/footer.view.test.tsx",
    description: "RunVariantSelectBody reasoning-effort picker (Default + high/minimal, current=high)",
    componentClass: "mini-footer-panel",
    variants: [
      { width: 80, height: RUN_COMMAND_PANEL_ROWS },
      { width: 160, height: RUN_COMMAND_PANEL_ROWS },
    ],
    mount: async (variant) => {
      const [variants] = createSignal(["high", "minimal"])
      const [current] = createSignal<string | undefined>("high")
      const app = await testRender(
        () => (
          <box width="100%" height={RUN_COMMAND_PANEL_ROWS}>
            <RunVariantSelectBody
              theme={() => RUN_THEME_FALLBACK.footer}
              variants={variants}
              current={current}
              onClose={() => {}}
              onSelect={() => {}}
            />
          </box>
        ),
        { width: variant.width, height: variant.height },
      )
      await app.renderOnce()
      await app.renderOnce()
      return app
    },
  },
  {
    id: "footer-settings",
    source: "test/mini/footer.view.test.tsx",
    description: "RunSettingsBody verbosity/thinking/tools/shell settings panel (mono=false, animations off)",
    componentClass: "mini-footer-panel",
    variants: [
      { width: 80, height: RUN_COMMAND_PANEL_ROWS },
      { width: 160, height: RUN_COMMAND_PANEL_ROWS },
    ],
    mount: async (variant) => {
      const [settings] = createSignal(resolveMiniSettings())
      const app = await testRender(
        () => (
          <box width="100%" height="100%">
            <RunSettingsBody
              theme={() => RUN_THEME_FALLBACK.footer}
              settings={settings}
              onClose={() => {}}
              onChange={() => {}}
              mono={false}
              animations={false}
            />
          </box>
        ),
        { width: variant.width, height: variant.height },
      )
      await app.renderOnce()
      await app.renderOnce()
      return app
    },
  },
]

export type { RunTheme }
