/**
 * Golden-frame exporter for the opencode v2.0.24 TUI.
 *
 * Drives the reference opentui renderer headless via its own test fixture
 * harness (testRender from @opentui/solid), renders a curated set of
 * representative scenarios at two widths, and serializes each frame as
 *   (a) a text grid  — exact captureCharFrame() bytes
 *   (b) an ANSI byte stream — canonical per-span SGR serialization (frame.ts)
 * plus, for width-edge scenarios,
 *   (c) a widths sidecar — the same probe strings measured with the
 *       `string-width` build bundled by @opentui/core (the renderer's
 *       "unicode" width method; ambiguous chars narrow, emoji 2 cells).
 *
 * CORPUS THEME (gate-8 hard precondition P1): every scenario mounts under
 * the reference's default theme — the built-in `opencode` v2 theme resolved
 * in dark mode (see scenario.ts CORPUS_THEME). The pre-P1 corpus (mixed
 * style bases) is kept under goldens/tui-frames/legacy-mixed-theme/.
 *
 * Each scenario is mounted TWICE per run and the two captures must be
 * byte-identical (in-run determinism). `--verify` re-renders everything,
 * re-measures the sidecars, and compares SHA-256s against the existing
 * manifest (cross-run determinism).
 *
 * Usage:
 *   bun /tmp/opencode/tui-dump/export.ts [--out DIR] [--only id ...] [--verify] [--json]
 */
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs"
import path from "node:path"

const REF_ROOT = process.env.OPENTUI_REF_ROOT ?? "/tmp/opencode/ref"

/** ensure node_modules symlinks before any @opentui import resolves */
async function ensureLinks() {
  const linkScript = path.join(import.meta.dir, "link_modules.ts")
  const { spawnSync } = await import("node:child_process")
  const result = spawnSync(process.execPath, [linkScript], { stdio: "inherit", env: { ...process.env, OPENTUI_REF_ROOT: REF_ROOT } })
  if (result.status !== 0) throw new Error("link_modules.ts failed")
}

interface CapturedVariant {
  width: number
  height: number
  rows: number
  cols: number
  txt: string
  ansi: string
}

type ScenarioTier = "tier-a" | "tier-b" | "tier-c1" | "width-edge"
type ComparisonDepth = "truecolor-canonical" | "degraded-256"

/** gate-8 approval table: tier → style comparison depth */
const TIER_COMPARISON_DEPTH: Record<ScenarioTier, ComparisonDepth> = {
  "tier-a": "truecolor-canonical",
  "tier-b": "degraded-256",
  "tier-c1": "truecolor-canonical",
  "width-edge": "truecolor-canonical",
}

interface ManifestEntry {
  id: string
  source: string
  description: string
  componentClass: string
  /** Acceptance tier (gate-8 approval table). */
  class: ScenarioTier
  /** Style comparison depth this scenario is judged at. */
  comparisonDepth: ComparisonDepth
  /**
   * Virtual-scroll scenarios (gate-9 addition a): the pinned scroll offset
   * this scenario's frame records; the `vscroll-*` family shares one
   * fixture, one scenario per offset.
   */
  scrollOffset?: "top" | "middle" | "bottom" | "stick-to-bottom"
  variants: {
    width: number
    height: number
    rows: number
    cols: number
    txt: { bytes: number; sha256: string }
    ansi: { bytes: number; sha256: string }
  }[]
  /** width-edge only: measured-width sidecar pinning string-width ground truth. */
  widths?: { file: string; bytes: number; sha256: string; samples: number }
}

/** Non-frame replay script entry (gate-8 non-frame acceptance track). */
interface ManifestReplayEntry {
  id: string
  description: string
  source: string
  keys: string
  steps: number
  file: string
  bytes: number
  sha256: string
}

interface Manifest {
  format: "tui-frame-golden/2"
  generator: string
  ansiFormat: string
  /** Corpus theme: every scenario renders under this theme (gate-8 P1). */
  theme: { id: string; label: string; mode: "dark" | "light" }
  /** Default style comparison depth when a scenario does not override. */
  comparisonDepthDefault: ComparisonDepth
  /** Set on superseded legacy corpora kept for diffing reference only. */
  superseded?: boolean
  scenarios: ManifestEntry[]
  /** Keypress-replay scripts (non-frame acceptance) under replay/. */
  replay?: ManifestReplayEntry[]
}

async function main() {
  const args = process.argv.slice(2)
  const verify = args.includes("--verify")
  const jsonOut = args.includes("--json")
  const outArg = args.findIndex((a) => a === "--out")
  const outDir = outArg >= 0 ? args[outArg + 1] : "/opt/opencode-rust/tools/parity/goldens/tui-frames"
  const onlyIdx = args.findIndex((a) => a === "--only")
  const only = onlyIdx >= 0 ? args[onlyIdx + 1].split(",") : undefined

  await ensureLinks()

  const { sha256 } = await import("./frame")
  const stringWidth = (await import("string-width")).default
  const scenarioMod = await import("./scenario")
  const cleanup = scenarioMod.cleanup
  const modules = [
    await import("./scenarios/tool_wrap"),
    await import("./scenarios/session_tabs"),
    await import("./scenarios/footer"),
    await import("./scenarios/session_ui"),
    await import("./scenarios/diff_viewer"),
    await import("./scenarios/width_edge"),
    await import("./scenarios/composer"),
    await import("./scenarios/markdown_frame"),
    await import("./scenarios/spinner_frozen"),
    await import("./scenarios/session_app"),
  ]
  const all = modules.flatMap((m) => m.scenarios)
  const selected = only ? all.filter((s) => only.includes(s.id)) : all
  if (selected.length === 0) throw new Error(`no scenarios matched: ${only?.join(", ")}`)

  console.log(`[tui-dump] exporting ${selected.length}/${all.length} scenarios × widths -> ${outDir}`)

  const entries: ManifestEntry[] = []
  const failures: { id: string; variant: string; error: unknown }[] = []

  for (const scenario of selected) {
    const tier: ScenarioTier = scenario.tier
    const entry: ManifestEntry = {
      id: scenario.id,
      source: scenario.source,
      description: scenario.description,
      componentClass: scenario.componentClass,
      class: tier,
      comparisonDepth: TIER_COMPARISON_DEPTH[tier],
      ...(scenario.scrollOffset ? { scrollOffset: scenario.scrollOffset } : {}),
      variants: [],
    }
    for (const variant of scenario.variants) {
      const tag = `${scenario.id}@${variant.width}x${variant.height}`
      try {
        const first = await capture(scenario, variant, cleanup)
        // determinism: second mount of the same scenario must be byte-identical
        const second = await capture(scenario, variant, cleanup)
        if (first.txt !== second.txt) throw new Error(`text frame not byte-stable across mounts for ${tag}`)
        if (first.ansi !== second.ansi) throw new Error(`ansi stream not byte-stable across mounts for ${tag}`)
        entry.variants.push({
          width: variant.width,
          height: variant.height,
          rows: first.rows,
          cols: first.cols,
          txt: { bytes: Buffer.byteLength(first.txt), sha256: sha256(first.txt) },
          ansi: { bytes: Buffer.byteLength(first.ansi), sha256: sha256(first.ansi) },
        })
        if (!verify) {
          mkdirSync(outDir, { recursive: true })
          writeFileSync(path.join(outDir, `${scenario.id}.w${variant.width}.txt`), first.txt)
          writeFileSync(path.join(outDir, `${scenario.id}.w${variant.width}.ansi`), first.ansi)
        }
        stagedFrames.set(tag, first)
        console.log(`[tui-dump]   ✓ ${tag} txt=${first.txt.length}B ansi=${first.ansi.length}B`)
      } catch (error) {
        failures.push({ id: scenario.id, variant: tag, error })
        console.error(`[tui-dump]   ✗ ${tag}: ${error instanceof Error ? error.message : String(error)}`)
      }
    }
    if (scenario.widthSamples) {
      const sidecar = widthsSidecar(scenario.id, scenario.widthSamples, stringWidth)
      stagedSidecars.set(scenario.id, sidecar)
      if (!verify) writeFileSync(path.join(outDir, `${scenario.id}.widths.json`), sidecar)
      entry.widths = {
        file: `${scenario.id}.widths.json`,
        bytes: Buffer.byteLength(sidecar),
        sha256: sha256(sidecar),
        samples: scenario.widthSamples.length,
      }
    }
    if (entry.variants.length > 0) entries.push(entry)
  }

  // Keypress-replay scripts (non-frame acceptance): deterministic JSON
  // fixtures, always written from the static module (no rendering).
  const replayEntries: ManifestReplayEntry[] = []
  const replayScripts = (await import("./replay_scripts")).replayScripts
  for (const script of replayScripts) {
    const bytes = `${JSON.stringify(script, null, 2)}\n`
    const file = `replay/${script.id}.json`
    if (!verify) {
      mkdirSync(path.join(outDir, "replay"), { recursive: true })
      writeFileSync(path.join(outDir, file), bytes)
    }
    replayEntries.push({
      id: script.id,
      description: script.description,
      source: script.source,
      keys: script.keys,
      steps: script.steps.length,
      file,
      bytes: Buffer.byteLength(bytes),
      sha256: sha256(bytes),
    })
    console.log(`[tui-dump]   ✓ replay ${script.id} steps=${script.steps.length}`)
  }

  const manifest: Manifest = {
    format: "tui-frame-golden/2",
    generator: "tui-dump (bun) @ opencode v2.0.24 reference",
    ansiFormat: "canonical span SGR: ESC[0;attrs;fg;bgm per style change, see frame.ts / tools/parity/src/tui_frame.rs",
    theme: { ...scenarioMod.CORPUS_THEME },
    comparisonDepthDefault: "truecolor-canonical",
    scenarios: entries,
    ...(replayEntries.length > 0 ? { replay: replayEntries } : {}),
  }

  if (failures.length > 0 && entries.length < 15) {
    console.error(`[tui-dump] FATAL: ${failures.length} variant failures left only ${entries.length} scenarios (need >= 15)`)
    process.exit(1)
  }

  if (verify) {
    const existing = JSON.parse(readFileSync(path.join(outDir, "manifest.json"), "utf8")) as Manifest
    let mismatch = 0
    for (const entry of existing.scenarios) {
      for (const variant of entry.variants) {
        const tag = `${entry.id}@${variant.width}x${variant.height}`
        const fresh = stagedFrames.get(tag)
        if (!fresh) {
          console.error(`[verify] ✗ ${tag}: not re-rendered`)
          mismatch++
          continue
        }
        const txtOk = sha256(fresh.txt) === variant.txt.sha256
        const ansiOk = sha256(fresh.ansi) === variant.ansi.sha256
        if (txtOk && ansiOk) console.log(`[verify] ✓ ${tag} byte-stable`)
        else {
          console.error(`[verify] ✗ ${tag} diverged (txt=${txtOk} ansi=${ansiOk})`)
          mismatch++
        }
      }
      if (entry.widths) {
        const fresh = stagedSidecars.get(entry.id)
        const onDisk = existsSync(path.join(outDir, entry.widths.file))
          ? readFileSync(path.join(outDir, entry.widths.file), "utf8")
          : undefined
        const ok = fresh !== undefined && onDisk === fresh && sha256(fresh) === entry.widths.sha256
        if (ok) console.log(`[verify] ✓ ${entry.id} widths sidecar stable`)
        else {
          console.error(`[verify] ✗ ${entry.id} widths sidecar diverged`)
          mismatch++
        }
      }
    }
    for (const replay of existing.replay ?? []) {
      const onDisk = existsSync(path.join(outDir, replay.file))
        ? readFileSync(path.join(outDir, replay.file), "utf8")
        : undefined
      const regenerated = replayScripts.find((script) => script.id === replay.id)
      const bytes = regenerated ? `${JSON.stringify(regenerated, null, 2)}\n` : undefined
      const ok = onDisk !== undefined && bytes !== undefined && onDisk === bytes && sha256(bytes) === replay.sha256
      if (ok) console.log(`[verify] ✓ replay ${replay.id} stable`)
      else {
        console.error(`[verify] ✗ replay ${replay.id} diverged`)
        mismatch++
      }
    }
    if (mismatch > 0) {
      console.error(`[verify] FAILED: ${mismatch} variants not byte-stable`)
      process.exit(1)
    }
    console.log(`[verify] OK: all variants byte-stable across runs`)
    return
  }

  mkdirSync(outDir, { recursive: true })
  const manifestBytes = `${JSON.stringify(manifest, null, 2)}\n`
  writeFileSync(path.join(outDir, "manifest.json"), manifestBytes)
  const summary = {
    theme: manifest.theme,
    scenarios: entries.length,
    variants: entries.reduce((n, e) => n + e.variants.length, 0),
    widthSidecars: entries.filter((e) => e.widths).length,
    replayScripts: replayEntries.length,
    failures: failures.map((f) => `${f.variant}: ${f.error instanceof Error ? f.error.message : String(f.error)}`),
    manifestSha256: sha256(manifestBytes),
  }
  console.log(jsonOut ? JSON.stringify(summary) : `[tui-dump] done: ${JSON.stringify(summary, null, 2)}`)
}

/** Deterministic measured-width sidecar for a width-edge scenario. */
function widthsSidecar(id: string, samples: string[], stringWidth: (s: string) => number): string {
  const sidecar = {
    id,
    measuredBy: "string-width@7.2.0 (bundled by @opentui/core; ambiguousIsNarrow=true, emoji=2)",
    widthMethod: "unicode",
    samples: samples.map((text) => ({
      text,
      utf16: text.length,
      codepoints: [...text].length,
      width: stringWidth(text),
    })),
  }
  return `${JSON.stringify(sidecar, null, 2)}\n`
}

const stagedFrames = new Map<string, CapturedVariant>()
const stagedSidecars = new Map<string, string>()
const { serializeAnsi } = await import("./frame")

async function capture(
  scenario: { mount: (v: { width: number; height: number }) => Promise<any> },
  variant: { width: number; height: number },
  cleanup: (app: any) => void,
): Promise<CapturedVariant> {
  const app = await scenario.mount(variant)
  try {
    const txt = app.captureCharFrame()
    const spans = app.captureSpans()
    const ansi = serializeAnsi(spans)
    return { width: variant.width, height: variant.height, rows: spans.rows, cols: spans.cols, txt, ansi }
  } finally {
    try {
      cleanup(app)
    } catch {
      app.renderer.destroy()
    }
  }
}

await main()
