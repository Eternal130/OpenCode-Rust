// Config-parse differential driver (ref side).
// Staged by tools/diff/config_parse.py into /tmp/opencode/ with a
// node_modules symlink to /opt/opencode-ref/node_modules so bare
// specifiers resolve inside the ref workspace. REF_ROOT env points at
// the ref checkout. Mirrors packages/core/src/config.ts parseInfo:
//   jsonc-parser parse(allowTrailingComma) -> ConfigNormalize.normalize ->
//   Schema.decodeUnknownOption(Info, {errors:"all",onExcessProperty:"ignore",
//   propertyOrder:"original"})
import { readdirSync, readFileSync, writeFileSync } from "node:fs"
import { parse } from "jsonc-parser"
import { Schema } from "effect"
import { Info } from "@opencode/schema/config"

const REF = process.env.DIFF_REF_ROOT!
const { normalize } = await import(`${REF}/packages/core/src/config/normalize.js`)

const args = process.argv.slice(2)
const get = (flag: string) => {
  const i = args.indexOf(flag)
  return i === -1 ? undefined : args[i + 1]
}
const corpus = get("--corpus")!
const out = get("--out")!

const decodeOptions = { errors: "all", onExcessProperty: "ignore", propertyOrder: "original" } as const
const decodeInfo = Schema.decodeUnknownOption(Info, decodeOptions)

const results: unknown[] = []
for (const name of readdirSync(corpus).sort()) {
  if (!name.endsWith(".json") && !name.endsWith(".jsonc")) continue
  const text = readFileSync(`${corpus}/${name}`, "utf8")
  const errors: unknown[] = []
  const input = parse(text, errors as never[], { allowTrailingComma: true })
  const parseOk = errors.length === 0 && input !== undefined
  const rec: Record<string, unknown> = { name, json_parse_ok: parseOk }
  if (parseOk) {
    const norm = normalize(input) as
      | { type: "normalized"; encoded: unknown; diagnostics: { kind: string; path: readonly string[] }[] }
      | { type: "rejected"; diagnostics: { kind: string; path: readonly string[] }[] }
    rec.normalize = norm.type
    rec.diagnostics = norm.diagnostics.map((d) => ({ kind: d.kind, path: d.path }))
    if (norm.type === "normalized") {
      const decoded = decodeInfo(norm.encoded) as { _tag: string } | undefined
      rec.encoded = norm.encoded
      rec.decode_ok = decoded?._tag === "Some"
    } else {
      rec.decode_ok = false
    }
  } else {
    rec.normalize = "not-reached"
    rec.diagnostics = []
    rec.decode_ok = false
  }
  results.push(rec)
}
writeFileSync(out, JSON.stringify(results))
console.log(`ref driver: ${results.length} cases -> ${out}`)
