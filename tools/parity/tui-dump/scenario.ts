import type { TestRendererSetup } from "@opentui/solid"

export interface ScenarioVariant {
  width: number
  height: number
}

/**
 * Acceptance tier this scenario belongs to (oracle-gate8 approval table).
 *
 * - `tier-a`: text grid **and** canonical ANSI stream compared byte-exact
 *   (SHA-256, both widths) in truecolor canonical form;
 * - `tier-b`: text grid byte-exact; styles compared after 256-color
 *   degradation (exact-hue drift tolerated);
 * - `tier-c1`: text grid byte-exact only (frozen frames);
 * - `width-edge`: display-width boundary probes (CJK / emoji ZWJ / combining
 *   marks); frames plus a measured-width JSON sidecar per scenario.
 */
export type ScenarioTier = "tier-a" | "tier-b" | "tier-c1" | "width-edge"

export interface Scenario {
  id: string
  /** Reference test file this scenario is derived from (repo-relative). */
  source: string
  description: string
  /** Component class used to bucket Phase 7 acceptance criteria. */
  componentClass: string
  /** Acceptance tier recorded in the manifest (gate-8 approval table). */
  tier: ScenarioTier
  variants: ScenarioVariant[]
  /**
   * Width-probe strings (width-edge scenarios only). Each string is rendered
   * as a row AND measured with the same `string-width` build @opentui/core
   * bundles, producing a `<id>.widths.json` sidecar.
   */
  widthSamples?: string[]
  /**
   * Mount the scenario in a fresh renderer at the given size and wait until
   * the frame is visually settled (no pending async boot work).
   */
  mount: (variant: ScenarioVariant) => Promise<TestRendererSetup>
}

export const REF = "/tmp/opencode/ref/packages/tui"

/**
 * The corpus theme (gate-8 hard precondition P1): every scenario renders
 * under the reference's default theme — the built-in `opencode` v2 theme
 * (`src/theme/assets/v2/opencode.json`) resolved in dark mode. Mounted
 * through `ThemeProvider mode="dark" source={emptyThemeSource}` (which
 * resolves `allThemes().opencode`) or, for the mini/ run views, through
 * `RUN_THEME_FALLBACK` (the same document resolved headless).
 */
export const CORPUS_THEME = { id: "opencode", label: "v2-dark", mode: "dark" } as const

export function cleanup(app: TestRendererSetup) {
  try {
    app.renderer.currentFocusedRenderable?.blur()
    app.renderer.currentFocusedEditor?.blur()
  } catch {
    /* renderer already torn down */
  }
  app.renderer.destroy()
}

export const WIDE_VARIANTS = (height: number): ScenarioVariant[] => [
  { width: 80, height },
  { width: 160, height },
]
