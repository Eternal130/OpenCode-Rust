// /compact compaction-pipeline differential driver (ref side).
// Staged by tools/diff/responses_compaction.py into /tmp/opencode/ with a
// node_modules symlink into the ref workspace so bare specifiers resolve.
//
// Mirrors packages/ai/src/protocols/utils/responses-compaction.ts:
//   - Response schema (lines 41-81): reconstructed 1:1 from the exported
//     OpenResponses schemas (CompactionItem, OpenResponsesReasoningItem,
//     Input/Output text, InputImage, InputFile, MessagePhase, Usage) — the
//     `Response` schema itself is module-private in the ref, so the driver
//     reassembles the same union from exported pieces (declared uncertainty
//     in the report).
//   - make()'s decode order (lines 140-148): decode full Response first,
//     then require a checkpoint item.
//   - toMessage (lines 151-188) + replayMedia (191-193): ported verbatim.
//
// Normalization (declared): providerMetadata objects drop null/undefined
// fields (rust writes explicit JSON nulls where TS drops undefined keys);
// usage raw passthrough (providerMetadata) is stripped as generated-source
// noise; message id/native are not produced by either side here.
import { readdirSync, readFileSync, writeFileSync } from "node:fs"
import { Schema } from "effect"

const REF = process.env.DIFF_REF_ROOT!
const OpenResponses = await import(`${REF}/packages/ai/src/protocols/open-responses.js`)
const { Media } = await import(`${REF}/packages/ai/src/media.js`)
const { optionalNull, JsonObject } = await import(`${REF}/packages/ai/src/protocols/shared.js`)
const { Message, CompactionPart } = await import(`${REF}/packages/ai/src/schema/index.js`)

const args = process.argv.slice(2)
const get = (flag: string) => {
  const i = args.indexOf(flag)
  return i === -1 ? undefined : args[i + 1]
}
const corpus = get("--corpus")!
const out = get("--out")!

const decodeOptions = { errors: "all", onExcessProperty: "ignore", propertyOrder: "original" } as const

// responses-compaction.ts:41-59
const Text = Schema.Union([OpenResponses.OpenResponsesInputText, OpenResponses.OpenResponsesOutputText])
const File = Schema.Union([
  Schema.Struct({
    ...OpenResponses.OpenResponsesInputFile.fields,
    file_url: Schema.String,
    file_data: Schema.optional(Schema.Never),
  }),
  Schema.Struct({
    ...OpenResponses.OpenResponsesInputFile.fields,
    file_data: Schema.String,
    file_url: Schema.optional(Schema.Never),
  }),
])
const MessageFields = {
  type: Schema.Literal("message"),
  id: Schema.optional(Schema.String),
  status: Schema.optional(Schema.String),
  phase: Schema.optional(OpenResponses.MessagePhase),
}
// responses-compaction.ts:60-81
const Response = Schema.Struct({
  object: Schema.Literal("response.compaction"),
  output: Schema.Array(
    Schema.Union([
      OpenResponses.CompactionItem,
      OpenResponses.OpenResponsesReasoningItem,
      Schema.Struct({
        ...MessageFields,
        role: Schema.Literal("user"),
        content: Schema.Array(Schema.Union([Text, OpenResponses.OpenResponsesInputImage, File])).check(
          Schema.isMinLength(1),
        ),
      }),
      Schema.Struct({
        ...MessageFields,
        role: Schema.Literal("assistant"),
        content: Schema.Array(Text).check(Schema.isMinLength(1)),
      }),
    ]),
  ),
  usage: Schema.optional(Schema.StructWithRest(OpenResponses.OpenResponsesUsage, [JsonObject])),
})
// Outer shell probe to bucket object-level failures apart from item failures.
const Outer = Schema.Struct({ object: Schema.Literal("response.compaction"), output: Schema.Array(Schema.Unknown) })

const decodeSync = <A, I>(schema: Schema.Schema<A, I>, value: unknown): A | undefined => {
  const result = Schema.decodeUnknownOption(schema, decodeOptions)(value) as
    | { _tag: string }
    | undefined
  return (result as any)?._tag === "Some" ? ((result as any).value as A) : undefined
}

// responses-compaction.ts:151-188, verbatim port with `model` faked to match
// the rust test_support request (provider "test-provider", no metadata key
// override -> metadataKey() === "openresponses").
const model = { provider: "test-provider", route: {} } as never
const key = OpenResponses.metadataKey(model)

function toMessage(item: any): any {
  if (item.type === "compaction")
    return Message.assistant(
      CompactionPart.make({ provider: (model as any).provider, id: item.id ?? undefined, encrypted: item.encrypted_content }),
    )
  if (item.type === "reasoning") {
    const summary = item.summary.length ? item.summary : [{ text: "" }]
    return Message.assistant(
      summary.map((part: any) => ({
        type: "reasoning" as const,
        text: part.text,
        providerMetadata: { [key]: { itemId: item.id, reasoningEncryptedContent: item.encrypted_content } },
      })),
    )
  }
  return Message.make({
    role: item.role,
    providerMetadata: { [key]: { itemId: item.id, type: item.type, status: item.status, phase: item.phase } },
    content: item.content.map((part: any): any => {
      if (part.type === "input_text" || part.type === "output_text") return { type: "text", text: part.text }
      if (part.type === "input_image")
        return {
          type: "media",
          media: replayMedia(part.image_url, "image/*"),
          providerMetadata: part.detail === undefined ? undefined : { [key]: { detail: part.detail } },
        }
      return {
        type: "media",
        media: replayMedia(part.file_url === undefined ? part.file_data : part.file_url, "application/octet-stream"),
        filename: part.filename,
        providerMetadata: part.detail === undefined ? undefined : { [key]: { detail: part.detail } },
      }
    }),
  })
}

// responses-compaction.ts:191-193
const replayMedia = (value: string, fallbackType: string) =>
  Media.parseDataUrl(value) ??
  (/^https?:\/\//.test(value) ? Media.url(value, { mediaType: fallbackType }) : Media.base64(value, fallbackType))

// ---- normalization to the shared wire shape ----
const stripNulls = (value: any): any => {
  if (Array.isArray(value)) return value.map(stripNulls)
  if (value && typeof value === "object") {
    const out: Record<string, unknown> = {}
    for (const [k, v] of Object.entries(value)) if (v !== null && v !== undefined) out[k] = stripNulls(v)
    return out
  }
  return value
}

const normMeta = (meta: unknown): unknown | undefined =>
  meta === undefined ? undefined : stripNulls(meta)

const normMedia = (asset: any): any => {
  const s = asset.source
  if (s.type === "base64") return { source: "base64", mediaType: s.mediaType, data: s.data }
  if (s.type === "url") return { source: "url", mediaType: s.mediaType, url: s.url }
  return { source: s.type }
}

const normPart = (part: any): any => {
  if (part.type === "text") return { type: "text", text: part.text }
  if (part.type === "reasoning") return { type: "reasoning", text: part.text, providerMetadata: normMeta(part.providerMetadata) }
  if (part.type === "media")
    return {
      type: "media",
      media: normMedia(part.media),
      filename: part.filename,
      providerMetadata: normMeta(part.providerMetadata),
    }
  if (part.type === "compaction") return { type: "compaction", provider: part.provider, id: part.id, encrypted: part.encrypted }
  return { type: part.type }
}

const normUsage = (u: any) =>
  u === undefined
    ? undefined
    : {
        inputTokens: u.inputTokens,
        outputTokens: u.outputTokens,
        nonCachedInputTokens: u.nonCachedInputTokens,
        cacheReadInputTokens: u.cacheReadInputTokens,
        cacheWriteInputTokens: u.cacheWriteInputTokens,
        reasoningTokens: u.reasoningTokens,
        totalTokens: u.totalTokens,
      }

const results: unknown[] = []
for (const name of readdirSync(corpus).sort()) {
  if (!name.endsWith(".json")) continue
  const text = readFileSync(`${corpus}/${name}`, "utf8")
  const rec: Record<string, unknown> = { name }
  let parsed: any
  try {
    parsed = JSON.parse(text)
  } catch {
    rec.accept = false
    rec.reject = "json"
    results.push(rec)
    continue
  }
  const outer = decodeSync(Outer, parsed)
  if (outer === undefined) {
    rec.accept = false
    rec.reject = "object"
    results.push(rec)
    continue
  }
  const decoded = decodeSync(Response, parsed)
  if (decoded === undefined) {
    rec.accept = false
    rec.reject = "item-schema"
    results.push(rec)
    continue
  }
  if (!decoded.output.some((item: any) => item.type === "compaction")) {
    rec.accept = false
    rec.reject = "no-checkpoint"
    results.push(rec)
    continue
  }
  rec.accept = true
  rec.reject = null
  rec.messages = decoded.output.map((item: any) => {
    const message = toMessage(item)
    return { role: message.role, parts: message.content.map(normPart), providerMetadata: normMeta(message.providerMetadata) }
  })
  rec.usage = normUsage(OpenResponses.mapUsage(decoded.usage, key))
  results.push(rec)
}
writeFileSync(out, JSON.stringify(results))
console.log(`ref driver: ${results.length} cases -> ${out}`)
