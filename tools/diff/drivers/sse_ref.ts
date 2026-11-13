// SSE-store projection differential — REF driver (channel 3).
//
// Drives the reference's data projection layer
// (packages/client/src/solid/data.ts `createData`) headlessly from bun:
//
//   - a stub `api()` whose every method rejects (all invalidation-driven
//     refetches stay no-ops, so the projection is purely event-driven);
//   - a fake `event` bus whose `listen` handler we invoke directly with
//     each corpus event ({type, id, created, data});
//   - after EVERY event, the session's projected message rows
//     (result.session.message.list(sessionID)) are snapshotted and
//     normalized to the channel's shared row schema (see the python
//     orchestrator / report header for the declared normalizations).
//
// Output: JSON array [{name, events, snapshots: [rows...]}] to --out.

const REF = process.env.DIFF_REF_ROOT
if (!REF) throw new Error("DIFF_REF_ROOT required")
process.on("unhandledRejection", () => {})

const { readdirSync, readFileSync, writeFileSync } = await import("node:fs")

// A callable Proxy that keeps returning itself for any property access and
// rejects when called — api().session.message.get(...) → rejected promise.
const rejectStub: any = new Proxy(function () {}, {
  get(_t, prop) {
    if (prop === "then" || prop === Symbol.toPrimitive) return undefined
    return rejectStub
  },
  apply() {
    return Promise.reject(new Error("diff-driver stub api"))
  },
})

const { createData } = await import(`${REF}/packages/client/src/solid/data.ts`)

// -- shared normalization (mirrors the rust driver) -------------------------

const STREAMING_INPUT = "<streaming-input>"

const normInput = (state: any) => {
  if (typeof state.input === "string" || state.status === "streaming") return STREAMING_INPUT
  if (state.input == null) return {}
  const out: Record<string, string> = {}
  for (const [k, v] of Object.entries(state.input)) {
    if (v === null) continue
    out[k] = typeof v === "string" ? v : JSON.stringify(v)
  }
  return out
}

// `toolDisplayContent` (rust joins text/name/uri blocks, drops empties).
const joinContent = (content: any) => {
  if (!Array.isArray(content)) return null
  const text = content
    .map((b: any) => b?.text ?? b?.name ?? b?.uri)
    .filter((t: any) => typeof t === "string" && t.length > 0)
    .join("\n")
  return text.length > 0 ? text : null
}

const normPart = (part: any) => {
  if (part.type === "text") return { type: "text", text: part.text ?? "" }
  if (part.type === "reasoning")
    return { type: "reasoning", text: part.text ?? "", completed: part.time?.completed !== undefined }
  if (part.type === "tool")
    return {
      type: "tool",
      id: part.id,
      name: part.name ?? "",
      status: part.state?.status ?? "streaming",
      input: normInput(part.state ?? {}),
      metadata: part.state?.metadata ?? {},
      output: joinContent(part.state?.content),
      error: part.state?.error?.message ?? null,
    }
  return { type: part.type }
}

const normModel = (m: any) => (m?.providerID && m?.id ? `${m.providerID}/${m.id}` : null)

const normRow = (row: any) => {
  switch (row.type) {
    case "assistant":
      return {
        row: "assistant",
        agent: row.agent ?? null,
        model: normModel(row.model),
        finish: row.finish ?? null,
        open: row.time?.completed === undefined,
        parts: (row.content ?? []).map(normPart),
      }
    case "agent-switched":
      return { row: "agent-switched", agent: row.agent ?? null, previous: row.previous ?? null }
    case "model-switched":
      return { row: "model-switched", model: normModel(row.model) }
    case "location-switched":
      return { row: "location-switched", directory: row.location?.directory ?? null }
    case "system":
      return { row: "system", description: row.description ?? null }
    case "synthetic":
      return { row: "synthetic", description: row.description ?? null }
    case "shell":
      return {
        row: "shell",
        shellID: row.shellID ?? null,
        command: row.command ?? "",
        status: row.status ?? "running",
        exit: row.exit ?? null,
        output: row.output?.output ?? null,
      }
    case "compaction": {
      // M2-rescan fix: mirror the rust driver's compaction arm (both
      // drivers previously fell through to the bare-type fallback).
      const failed = row.status === "failed"
      const tokens = row.tokens
      return {
        row: "compaction",
        id: row.id ?? null,
        running: row.status === "running",
        failed,
        cancelled: failed && row.error?.type === "aborted",
        summary: row.summary ?? "",
        tokens_in:
          tokens === undefined
            ? null
            : (tokens.input ?? 0) + (tokens.cache?.read ?? 0) + (tokens.cache?.write ?? 0),
        tokens_out:
          tokens === undefined ? null : (tokens.output ?? 0) + (tokens.reasoning ?? 0),
        provider_compaction: row.providerContext !== undefined,
      }
    }
    default:
      return { row: row.type ?? "unknown" }
  }
}

// -- corpus run -------------------------------------------------------------

const args = process.argv.slice(2)
const flag = (name: string) => {
  const i = args.indexOf(`--${name}`)
  return i >= 0 ? args[i + 1] : undefined
}
const corpus = flag("corpus")!
const out = flag("out")!

const results: any[] = []
for (const file of readdirSync(corpus).filter((f) => f.endsWith(".json")).sort()) {
  const sequence = JSON.parse(readFileSync(`${corpus}/${file}`, "utf8"))
  let handler: ((e: { name: string; details: any }) => void) | undefined
  const data = createData({
    api: () => rejectStub,
    directory: "/tmp/opencode/diff-sse",
    event: {
      on: () => () => {},
      listen: (h: any) => {
        handler = h
        return () => {}
      },
    },
  })
  const snapshots: any[] = []
  let sessionID = ""
  for (const ev of sequence.events) {
    sessionID = ev.data?.sessionID ?? sessionID
    handler!({ name: ev.type, details: { type: ev.type, id: ev.id, created: ev.created, data: ev.data ?? {} } })
    snapshots.push(data.session.message.list(sessionID).map(normRow))
  }
  results.push({ name: sequence.name ?? file.replace(/\.json$/, ""), events: sequence.events.length, snapshots })
}

writeFileSync(out, JSON.stringify(results, null, 1))
console.log(`ref driver: ${results.length} sequences -> ${out}`)
