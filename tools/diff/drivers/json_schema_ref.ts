// JSON-Schema validator differential driver (ref side, channel 4).
// Staged by tools/diff/json_schema_validate.py into /tmp/opencode/ with a
// node_modules symlink to the ref workspace so bare imports resolve.
//
// Mirrors packages/core/src/tool/runtime.ts validateInput for raw
// JsonSchema inputs (the path MCP/plain schemas take): the Cache lookup
// builds a codec via jsonSchema() inside Effect.try — a build failure
// maps to `undefined`, which validateInput treats as ACCEPT. That
// catch-to-accept is part of the ref surface and is mirrored exactly:
// a schema whose codec fails to build accepts every instance.
//
//   jsonSchema()  — runtime.ts:101-107 (draft-07 vs 2020-12 selection)
//   decodeUnknownEffect — runtime.ts:78
import { readdirSync, readFileSync, writeFileSync } from "node:fs"
import { Effect, JsonSchema, Schema, SchemaRepresentation } from "effect"

const args = process.argv.slice(2)
const get = (flag: string) => {
  const i = args.indexOf(flag)
  return i === -1 ? undefined : args[i + 1]
}
const corpus = get("--corpus")!
const out = get("--out")!

const isRecord = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v)

// runtime.ts:101-107
const buildCodec = (schema: JsonSchema.JsonSchema) => {
  const draft =
    (typeof schema.$schema === "string" && schema.$schema.includes("draft-07")) || isRecord(schema) && "definitions" in schema
      ? JsonSchema.fromSchemaDraft07(schema as never)
      : JsonSchema.fromSchemaDraft2020_12(schema as never)
  return Schema.make<Schema.Codec<unknown>>(SchemaRepresentation.fromJsonSchemaDocument(draft).ast)
}

const results: unknown[] = []
for (const name of readdirSync(corpus).sort()) {
  if (!name.endsWith(".json")) continue
  const doc = JSON.parse(readFileSync(`${corpus}/${name}`, "utf8")) as {
    name: string
    keywords: string[]
    schema: unknown
    instances: { label: string; value: unknown }[]
  }
  // Cache lookup semantics (runtime.ts:9-18): build failure -> undefined -> accept-all.
  let codec: ReturnType<typeof buildCodec> | undefined
  let codec_ok = true
  try {
    codec = buildCodec(doc.schema as never)
  } catch {
    codec_ok = false
  }
  for (const inst of doc.instances) {
    let accept = true
    if (codec !== undefined) {
      accept = Effect.runSync(
        Schema.decodeUnknownEffect(codec)(inst.value, { errors: "first" } as never).pipe(
          Effect.match({ onFailure: () => false, onSuccess: () => true }),
        ),
      )
    }
    results.push({ name: `${doc.name}/${inst.label}`, case: doc.name, label: inst.label, keywords: doc.keywords, accept, codec_ok })
  }
}
writeFileSync(out, JSON.stringify(results))
console.log(`ref driver: ${results.length} pairs -> ${out}`)
