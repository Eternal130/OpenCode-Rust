/**
 * Self-healing node_modules for the golden-frame exporter.
 *
 * The exporter lives outside the reference checkout, so bare imports
 * (@opentui/*, solid-js, @opencode/*) need a node_modules next to it.
 * We symlink each package from the reference runtime at /tmp/opencode/ref,
 * preferring the tui workspace node_modules (patched solid-js, @opentui)
 * and falling back to the repo root node_modules.
 */
import { existsSync, mkdirSync, readdirSync, realpathSync, rmSync, statSync, symlinkSync } from "node:fs"
import path from "node:path"

const REF = process.env.OPENTUI_REF_ROOT ?? "/tmp/opencode/ref"
const TUI_NM = path.join(REF, "packages/tui/node_modules")
const ROOT_NM = path.join(REF, "node_modules")
const LOCAL_NM = path.join(import.meta.dir, "node_modules")

function isDir(p: string) {
  try {
    return statSync(p).isDirectory()
  } catch {
    return false
  }
}

function ensureLink(name: string) {
  const target = isDir(path.join(TUI_NM, name))
    ? path.join(TUI_NM, name)
    : isDir(path.join(ROOT_NM, name))
      ? path.join(ROOT_NM, name)
      : undefined
  if (!target) return false
  const link = path.join(LOCAL_NM, name)
  try {
    if (statSync(link).isSymbolicLink() && realpathSync(link) === target) return true
  } catch {
    /* missing or dangling — recreate below */
  }
  try {
    rmSync(link, { force: true, recursive: true })
  } catch {
    /* ignore */
  }
  mkdirSync(path.dirname(link), { recursive: true })
  symlinkSync(target, link, "dir")
  return true
}

mkdirSync(LOCAL_NM, { recursive: true })
for (const nm of [ROOT_NM, TUI_NM]) {
  if (!isDir(nm)) continue
  for (const entry of readdirSync(nm)) {
    if (entry.startsWith(".")) continue
    const names = entry.startsWith("@") ? readdirSync(path.join(nm, entry)).map((c) => `${entry}/${c}`) : [entry]
    for (const name of names) {
      if (!isDir(path.join(nm, name))) continue
      ensureLink(name)
    }
  }
}
console.log("[link_modules] node_modules links ensured")
