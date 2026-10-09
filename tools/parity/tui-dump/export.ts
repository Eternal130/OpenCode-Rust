/**
 * Golden-frame exporter for the opencode v2.0.24 TUI.
 *
 * Drives the reference opentui renderer headless via its own test fixture
 * harness (testRender from @opentui/solid), renders a curated set of
 * representative scenarios at two widths, and serializes each frame as
 *   (a) a text grid  — exact captureCharFrame() bytes
 *   (b) an ANSI byte stream — canonical per-span SGR serialization (frame.ts)
 *
 * Each scenario is mounted TWICE per run and the two captures must be
 * byte-identical (in-run determinism). `--verify` re-renders everything and
 * compares SHA-256s against the existing manifest (cross-run determinism).
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

interface ManifestEntry {
  id: string
  source: string
  description: string
  componentClass: string
  variants: {
    width: number
    height: number
    rows: number
    cols: number
    txt: { bytes: number; sha256: string }
    ansi: { bytes: number; sha256: string }
  }[]
}

interface Manifest {
  format: "tui-frame-golden/1"
  generator: string
  ansiFormat: string
  scenarios: ManifestEntry[]
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

  const { sha256, serializeAnsi } = await import("./frame")
  const scenarioMod = await import("./scenario")
  const cleanup = scenarioMod.cleanup
  const modules = [
    await import("./scenarios/tool_wrap"),
    await import("./scenarios/session_tabs"),
    await import("./scenarios/footer"),
    await import("./scenarios/session_ui"),
    await import("./scenarios/diff_viewer"),
  ]
  const all: Scenario[] = modules.flatMap((m) => m.scenarios)
  const selected = only ? all.filter((s) => only.includes(s.id)) : all
  if (selected.length === 0) throw new Error(`no scenarios matched: ${only?.join(", ")}`)

  console.log(`[tui-dump] exporting ${selected.length}/${all.length} scenarios × widths -> ${outDir}`)

  const entries: ManifestEntry[] = []
  const failures: { id: string; variant: string; error: unknown }[] = []

  for (const scenario of selected) {
    const entry: ManifestEntry = {
      id: scenario.id,
      source: scenario.source,
      description: scenario.description,
      componentClass: scenario.componentClass,
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
        staged.set(tag, first)
        console.log(`[tui-dump]   ✓ ${tag} txt=${first.txt.length}B ansi=${first.ansi.length}B`)
      } catch (error) {
        failures.push({ id: scenario.id, variant: tag, error })
        console.error(`[tui-dump]   ✗ ${tag}: ${error instanceof Error ? error.message : String(error)}`)
      }
    }
    if (entry.variants.length > 0) entries.push(entry)
  }

  const manifest: Manifest = {
    format: "tui-frame-golden/1",
    generator: "tui-dump (bun) @ opencode v2.0.24 reference",
    ansiFormat: "canonical span SGR: ESC[0;attrs;fg;bgm per style change, see frame.ts / tools/parity/src/tui_frame.rs",
    scenarios: entries,
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
        const fresh = staged.get(tag)
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
    scenarios: entries.length,
    variants: entries.reduce((n, e) => n + e.variants.length, 0),
    failures: failures.map((f) => `${f.variant}: ${f.error instanceof Error ? f.error.message : String(f.error)}`),
    manifestSha256: sha256(manifestBytes),
  }
  console.log(jsonOut ? JSON.stringify(summary) : `[tui-dump] done: ${JSON.stringify(summary, null, 2)}`)
}

const staged = new Map<string, CapturedVariant>()
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
