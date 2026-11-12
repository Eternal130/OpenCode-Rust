// ACP-translate differential driver (ref side).
// Staged by tools/diff/acp_translate.py into /tmp/opencode/diff-acp with a
// node_modules symlink to $REF/packages/cli/node_modules so bare
// specifiers resolve. Imports the ref fold pipeline directly:
//   packages/cli/src/acp/translate.ts (fold/response/abandon/initial)
//
// Case file shape (see tools/diff/gen_acp_corpus.py):
//   { name, ctx, events:[{id?, type, data}], abandon? }
// Output record (normalized, matching the rust driver byte-for-byte
// after the python orchestrator's canon()):
//   { name, events: [{outputs: [...], terminal, response?}],
//     abandon_outputs?: [...] }
import { readdirSync, readFileSync, writeFileSync } from "node:fs"

const REF = process.env.DIFF_REF_ROOT!
const T = await import(`${REF}/packages/cli/src/acp/translate.js`)

const args = process.argv.slice(2)
const get = (flag: string) => {
  const i = args.indexOf(flag)
  return i === -1 ? undefined : args[i + 1]
}
const corpus = get("--corpus")!
const out = get("--out")!

function normTool(tool: any) {
  if (!tool) return undefined
  return { sessionID: tool.sessionID, id: tool.id, name: tool.name,
           input: tool.input, metadata: tool.metadata }
}
function normChild(child: any) {
  if (!child) return undefined
  return { id: child.id, parentID: child.parentID, depth: child.depth,
           ...(child.title !== undefined ? { title: child.title } : {}) }
}
function normOutput(output: any) {
  switch (output._tag) {
    case "SessionUpdate":
      return { tag: "session_update", update: output.update }
    case "ChildUpdate":
      return { tag: "child_update", update: output.update }
    case "PermissionAsk":
      return {
        tag: "permission_ask",
        data: output.event.data,
        tool: normTool(output.tool) ?? null,
        child: normChild(output.child) ?? null,
      }
    case "FormAsk":
      return {
        tag: "form_ask",
        form: output.form,
        child: normChild(output.child) ?? null,
      }
    case "AskSettled":
      return { tag: "ask_settled", id: output.id }
    default:
      return { tag: "unknown" }
  }
}

const results: unknown[] = []
for (const name of readdirSync(corpus).sort()) {
  if (!name.endsWith(".json")) continue
  const cas = JSON.parse(readFileSync(`${corpus}/${name}`, "utf8"))
  const ctx = cas.ctx
  let state = T.initial
  const events: unknown[] = []
  for (const event of cas.events) {
    let folded
    try {
      folded = T.fold(state, event, ctx)
    } catch (error) {
      events.push({ outputs: [{ tag: "driver_error", message: String(error) }],
                    terminal: null })
      continue
    }
    state = folded.state
    events.push({
      outputs: folded.outputs.map(normOutput),
      terminal: folded.terminal ?? null,
      ...(folded.terminal
        ? { response: T.response(state, ctx.sessionID, folded.terminal) }
        : {}),
    })
  }
  const rec: Record<string, unknown> = { name: cas.name, events }
  if (cas.abandon) {
    const abandoned = T.abandon(state, ctx)
    rec.abandon_outputs = abandoned.outputs.map(normOutput)
  }
  results.push(rec)
}
writeFileSync(out, JSON.stringify(results))
console.log(`ref acp driver: ${results.length} cases -> ${out}`)
