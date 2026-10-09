import type { TestRendererSetup } from "@opentui/solid"

export interface ScenarioVariant {
  width: number
  height: number
}

export interface Scenario {
  id: string
  /** Reference test file this scenario is derived from (repo-relative). */
  source: string
  description: string
  /** Component class used to bucket Phase 7 acceptance criteria. */
  componentClass: string
  variants: ScenarioVariant[]
  /**
   * Mount the scenario in a fresh renderer at the given size and wait until
   * the frame is visually settled (no pending async boot work).
   */
  mount: (variant: ScenarioVariant) => Promise<TestRendererSetup>
}

export const REF = "/tmp/opencode/ref/packages/tui"

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
