/** @jsxImportSource @opentui/solid */
import { testRender } from "@opentui/solid"
import { For } from "solid-js"
import { InlineToolRow } from "/tmp/opencode/ref/packages/tui/src/routes/session/index"
import { cleanup, WIDE_VARIANTS, type Scenario } from "../scenario"

type ToolFixture = { icon: string; label: string; error?: string }

const tools: readonly ToolFixture[] = [
  {
    icon: "✱",
    label:
      'Grep "OPENCODE.*DB|database|sqlite|drizzle|dev.*db|data.*dir|xdg|APPDATA" in packages/opencode/src (151 matches)',
  },
  {
    icon: "✱",
    label: 'Glob "**/*db*" in packages/opencode (6 matches)',
  },
  {
    icon: "→",
    label: "Read packages/opencode/src/storage/db.ts [offset=1, limit=130]",
  },
  {
    icon: "→",
    label: "Read packages/opencode/src/index.ts [offset=1, limit=100]",
    error: "No LSP server available for this file type.",
  },
  {
    icon: "✱",
    label:
      'Grep "export const OPENCODE_DB|OPENCODE_DB|OPENCODE_DEV|Global\\.Path\\.data|data =" in packages/opencode/src (115 matches)',
  },
] as const

function ToolList(props: { errorExpanded?: boolean }) {
  return (
    <box flexDirection="column" width={72}>
      <box flexDirection="column">
        <For each={tools}>
          {(item) => (
            <InlineToolRow
              icon={item.icon}
              complete={true}
              pending=""
              failed={Boolean(item.error)}
              error={item.error}
              errorExpanded={props.errorExpanded}
            >
              {item.label}
            </InlineToolRow>
          )}
        </For>
      </box>
    </box>
  )
}

async function mount(props: { errorExpanded?: boolean }, variant: { width: number; height: number }) {
  const app = await testRender(() => <ToolList errorExpanded={props.errorExpanded} />, variant)
  await app.renderOnce()
  await app.renderOnce()
  return app
}

export const scenarios: Scenario[] = [
  {
    id: "tool-wrap-rows",
    source: "test/cli/tui/inline-tool-wrap-snapshot.test.tsx",
    description: "Five consecutive grep/glob/read InlineToolRow summaries at width 72 (wrap behavior)",
    componentClass: "tool-row",
    variants: WIDE_VARIANTS(12),
    mount: (variant) => mount({}, variant)
  },
  {
    id: "tool-wrap-error-expanded",
    source: "test/cli/tui/inline-tool-wrap-snapshot.test.tsx",
    description: "Same tool rows with errorExpanded (LSP error rendered under the tool text)",
    componentClass: "tool-row",
    variants: WIDE_VARIANTS(12),
    mount: (variant) => mount({ errorExpanded: true }, variant)
  },
]
