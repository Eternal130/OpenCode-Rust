/**
 * Markdown frame goldens — gate-8 approval tier C1 (text grid byte-exact;
 * style/ANSI parity deferred to C2), gate-9 finding 2's instrument.
 *
 * Mounts the same `<markdown>` renderable `TextPart` drives in
 * `routes/session/message-parts.tsx` (identical props: theme.markdown.text
 * fg, theme.background.base bg, top-level internal block mode, conceal in
 * rendered mode) so the frame is exactly what an assistant text part paints.
 *
 * The paragraph carries a bold run of 33 visible chars positioned so the wrap
 * boundary at BOTH pinned widths (80 and 40) falls inside the run — the
 * gate-9 instrument case: a styled span must wrap as one flat text stream,
 * never break at the span boundary.
 *
 * Markdown blocks fill asynchronously (incremental tree-sitter passes), so
 * the mount waits for the final block text before capturing.
 */
/** @jsxImportSource @opentui/solid */
import { testRender } from "@opentui/solid"
import { addDefaultParsers } from "@opentui/core"
import parsers from "/tmp/opencode/ref/packages/tui/src/parsers-config"
import { ConfigProvider } from "/tmp/opencode/ref/packages/tui/src/config/index.tsx"
import { ThemeProvider, useTheme, useThemes } from "/tmp/opencode/ref/packages/tui/src/context/theme"
import { emptyThemeSource } from "/tmp/opencode/ref/packages/tui/test/fixture/fixture"
import { createTuiResolvedConfig } from "/tmp/opencode/ref/packages/tui/test/fixture/tui-runtime"
import type { Scenario, ScenarioVariant } from "../scenario"

addDefaultParsers(parsers.parsers)

const MARKDOWN = [
  "## Parser rewrite",
  "",
  "The parser now handles **bold-spanning-the-wrap-boundary-abcdefghij** as one styled run.",
  "",
  "Second paragraph opens with filler words and then **bold-run-deliberately-longer-than-the-wrap-boundary-at-eighty** before the tail.",
  "",
  "Ordinary paragraph with `inline.code` and *emphasis* mixed into prose.",
  "",
  "- item one with `inline.code`",
  "- item two follows the first",
  "- item three closes the list",
  "",
  "1. ordered entry",
  "2. second ordered entry",
  "",
  "```ts",
  "const x = parse(input)",
  "return x.map(transform)",
  "```",
].join("\n")

function MarkdownBody() {
  const theme = useTheme()
  const { currentSyntax: syntax } = useThemes()
  return (
    <box paddingLeft={3} flexShrink={0} width="100%">
      <markdown
        syntaxStyle={syntax()}
        content={MARKDOWN}
        streaming={false}
        internalBlockMode="top-level"
        tableOptions={{ style: "grid", cellPaddingX: 1 }}
        conceal={true}
        fg={theme.markdown.text}
        bg={theme.background.base}
      />
    </box>
  )
}

async function mount(variant: ScenarioVariant) {
  const config = createTuiResolvedConfig()
  const app = await testRender(
    () => (
      <ConfigProvider config={config}>
        <ThemeProvider mode="dark" source={emptyThemeSource}>
          <MarkdownBody />
        </ThemeProvider>
      </ConfigProvider>
    ),
    { ...variant, kittyKeyboard: true },
  )
  app.renderer.start()
  // Blocks fill over successive render passes; wait for the last block.
  await app.waitForFrame(
    (frame) => frame.includes("Parser rewrite") && frame.includes("transform") && frame.includes("item three"),
    { maxPasses: 200 },
  )
  await app.waitForVisualIdle()
  await app.renderOnce()
  return app
}

export const scenarios: Scenario[] = [
  {
    id: "markdown-frame",
    source: "packages/tui/src/routes/session/message-parts.tsx (TextPart mount)",
    description:
      "Assistant markdown mix (heading/bold/inline-code/emphasis/lists/fence) with a ≥10-char bold run crossing the wrap boundary at both widths",
    componentClass: "markdown",
    tier: "tier-c1",
    variants: [
      { width: 80, height: 30 },
      { width: 40, height: 30 },
    ],
    mount,
  },
]
