/**
 * R-04 app-archive packager (release pipeline `web-assets` job; also the
 * local browser-smoke path, see tools/RELEASE.md § web-ui).
 *
 *   bun tools/web-assets/package.ts --dist <dir> --ref <commit> [--out app-archive.json]
 *
 * Walks a built app dist (anomalyco/opencode `packages/app/dist` at the
 * pinned ref), brotli-compresses every file (node:zlib, quality 6 — the
 * same level the Rust tests pin via `brotli6`), base64-encodes the
 * packed bytes, and writes the self-describing archive JSON that
 * oc-server embeds at build time (`OPENCODE_APP_ARCHIVE`) or loads at
 * runtime (`OPENCODE_APP_ASSETS`):
 *
 * {
 *   "format": "opencode-rust-app-archive/1",
 *   "provenance": { "ref_commit": "...", "built_at": "...", "app_commit": "..." },
 *   "files": { "<path>": "<base64(brotli(bytes))>" }
 * }
 *
 * Exits non-zero on anything suspicious (missing dist, missing
 * index.html) — the release must never embed a half-built app.
 */
import { readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { brotliCompressSync, constants as ZlibConstants } from "node:zlib";

function arg(name: string): string | undefined {
  const index = process.argv.indexOf(`--${name}`)
  return index >= 0 ? process.argv[index + 1] : undefined
}

const dist = resolve(arg("dist") ?? "")
const ref = arg("ref") ?? "unknown"
const out = resolve(arg("out") ?? "app-archive.json")

function fail(message: string): never {
  console.error(`package.ts: ${message}`)
  process.exit(1)
}

function walk(dir: string): string[] {
  const files: string[] = []
  for (const entry of readdirSync(dir, { withFileTypes: true }).sort((a, b) =>
    a.name.localeCompare(b.name),
  )) {
    const full = join(dir, entry.name)
    if (entry.isDirectory()) files.push(...walk(full))
    else if (entry.isFile()) files.push(relative(dist, full).split("\\").join("/"))
    else fail(`unexpected dirent (not file, not dir): ${full}`)
  }
  return files
}

if (!statSync(dist, { throwIfNoEntry: false })?.isDirectory()) {
  fail(`--dist ${dist} is not a directory (did the app build run?)`)
}
const paths = walk(dist)
if (paths.length === 0 || !paths.includes("index.html")) {
  fail(`${dist} does not look like a built app dist (expected index.html among ${paths.length} files)`)
}

const files: Record<string, string> = {}
let plainBytes = 0
let packedBytes = 0
for (const path of paths) {
  const plain = readFileSync(join(dist, path))
  const packed = brotliCompressSync(plain, {
    params: { [ZlibConstants.BROTLI_PARAM_QUALITY]: 6 },
  })
  plainBytes += plain.length
  // base64 inflates by 4/3 — report the on-the-wire contribution.
  packedBytes += Math.ceil((packed.length * 4) / 3)
  files[path] = packed.toString("base64")
}

const archive = {
  format: "opencode-rust-app-archive/1",
  provenance: {
    ref_commit: ref,
    built_at: new Date().toISOString(),
    app_commit: ref,
  },
  files,
}
writeFileSync(out, JSON.stringify(archive))
console.log(
  `package.ts: ${paths.length} files, ${plainBytes} B plain -> ~${packedBytes} B packed -> ${statSync(out).size} B archive at ${out}`,
)
