/** @jsxImportSource @opentui/solid */
import { testRender } from "@opentui/solid"
import { For } from "solid-js"
import { ConfigProvider } from "/tmp/opencode/ref/packages/tui/src/config/index.tsx"
import { ThemeProvider, useTheme } from "/tmp/opencode/ref/packages/tui/src/context/theme"
import { emptyThemeSource } from "/tmp/opencode/ref/packages/tui/test/fixture/fixture"
import { createTuiResolvedConfig } from "/tmp/opencode/ref/packages/tui/test/fixture/tui-runtime"
import type { Scenario, ScenarioVariant } from "../scenario"

/**
 * Display-width boundary probes (oracle-gate8 finding 2).
 *
 * Each scenario renders one sample string per row inside a wrapping box of
 * `min(width - 8, 72)` columns, at terminal widths 80 and 40 — so every
 * sample wraps at two different boundaries. The frames pin opentui's actual
 * cell layout for:
 *
 * - CJK text (mixed zh + ASCII, full-width punctuation, wrap-anywhere);
 * - emoji + ZWJ sequences + skin-tone modifiers (grapheme clusters);
 * - combining marks and East-Asian-ambiguous-width characters.
 *
 * The same strings are measured with the `string-width` build bundled by
 * `@opentui/core` (see export.ts) and pinned as `<id>.widths.json` sidecars
 * so the Rust `display_width` can be unit-tested against ground truth
 * independently of the frames.
 */

const CJK_SAMPLES = [
  "搜索 packages/opencode/src 中的数据库配置（共 151 处匹配）",
  "读取 packages/opencode/src/index.ts ［offset=1, limit=100］成功",
  "错误：数据库连接失败，请检查网络后重试。",
  "中文没有空格时可以在任意字符之间换行——此时最长词的宽度为一格",
  "全角标点：，。！？；：「」『』（）《》……——",
  "混排 abc123 与中文、日本語とハングル 한국어 交替",
  "ＡＢＣ１２３ fullwidth letters/digits ｛｝［］",
  "重复长句触发换行：数据库连接池已满，任务队列积压，请扩容后重试",
]

const EMOJI_SAMPLES = [
  "thumbs: 👍 plain and 👍🏽 medium skin tone",
  "family ZWJ: 👨‍👩‍👧‍👦 four members",
  "rainbow flag: 🏳️‍🌈 ZWJ + VS16",
  "astronaut: 🧑🏻‍🚀 skin tone + ZWJ",
  "handshake: 🫱🏻‍🫲🏿 two skin tones",
  "shipped 🎉✅🚀 three files 💯 percent done",
  "hearts: ❤️‍🔥 beating and 💞 sparkling",
  "keycap 1️⃣ vs circled ⑩ digits",
]

const COMBINING_SAMPLES = [
  "precomposed: école déjà naïve café",
  "NFD: e\u0301cole de\u0301ja\u0300 a\u0300 co\u0302te\u0302 (e + U+0301)",
  "ambiguous narrow: ±0.5 · ratio · ellipsis… · ○ △ ▽ ※",
  "circled/roman: ① ② ③ ⅓ Ⅷ Ⅻ Ⅺ",
  "box drawing: ─│┌┐└┘ ├┤┬┴┼ ═ ║ □ ■",
  "devanagari: हिन्दी में लिखा matras combine",
  "thai: สวัสดีครับ vowel marks above",
  "flags: 🇺🇸🇯🇵🇩🇪 regional indicator pairs",
]

function Samples(props: { samples: string[]; contentWidth: number }) {
  const theme = useTheme()
  return (
    <box flexDirection="column">
      <For each={props.samples}>
        {(sample) => (
          <box paddingLeft={3} width={props.contentWidth}>
            <text fg={theme.text.base}>{sample}</text>
          </box>
        )}
      </For>
    </box>
  )
}

async function mountSamples(samples: string[], variant: ScenarioVariant) {
  const config = createTuiResolvedConfig()
  const contentWidth = Math.min(variant.width - 8, 72)
  const app = await testRender(
    () => (
      <ConfigProvider config={config}>
        <ThemeProvider mode="dark" source={emptyThemeSource}>
          <Samples samples={samples} contentWidth={contentWidth} />
        </ThemeProvider>
      </ConfigProvider>
    ),
    variant,
  )
  await app.renderOnce()
  await app.renderOnce()
  return app
}

const WIDTH_EDGE_VARIANTS: ScenarioVariant[] = [
  { width: 80, height: 30 },
  { width: 40, height: 30 },
]

export const scenarios: Scenario[] = [
  {
    id: "width-edge-cjk",
    source: "tui-dump width-edge corpus (oracle-gate8 finding 2)",
    description:
      "CJK display-width probe: mixed zh + ASCII, full-width punctuation and brackets, wrap-anywhere reflow",
    componentClass: "width-probe",
    tier: "width-edge",
    variants: WIDTH_EDGE_VARIANTS,
    widthSamples: CJK_SAMPLES,
    mount: (variant) => mountSamples(CJK_SAMPLES, variant),
  },
  {
    id: "width-edge-emoji",
    source: "tui-dump width-edge corpus (oracle-gate8 finding 2)",
    description:
      "Emoji display-width probe: ZWJ sequences (family/flags/handshake), skin-tone modifiers, keycap VS16",
    componentClass: "width-probe",
    tier: "width-edge",
    variants: WIDTH_EDGE_VARIANTS,
    widthSamples: EMOJI_SAMPLES,
    mount: (variant) => mountSamples(EMOJI_SAMPLES, variant),
  },
  {
    id: "width-edge-combining",
    source: "tui-dump width-edge corpus (oracle-gate8 finding 2)",
    description:
      "Combining-marks / ambiguous-width probe: NFD vs precomposed, ±·…○△, circled/roman numerals, box drawing, Indic scripts",
    componentClass: "width-probe",
    tier: "width-edge",
    variants: WIDTH_EDGE_VARIANTS,
    widthSamples: COMBINING_SAMPLES,
    mount: (variant) => mountSamples(COMBINING_SAMPLES, variant),
  },
]
