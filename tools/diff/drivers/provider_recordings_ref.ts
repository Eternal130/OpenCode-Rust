// Provider-protocol decode differential — REF driver (channel 7).
//
// Staged by tools/diff/provider_recordings.py into /tmp/opencode/diff-provrec
// with a node_modules symlink to $REF/packages/ai/node_modules. For every
// manifest case it drives the REAL packages/ai protocol step machine over
// the recorded response body, mirroring what the ref route client does
// (packages/ai/src/route/client.ts streamPrepared):
//
//   frames  = family framing (SSE variant) over the body bytes
//   events  = Schema.decodeUnknownEffect(protocol.stream.event) per frame
//   state   = protocol.stream.initial(request)
//   outputs = protocol.stream.step(state, event)* (+ terminal takeUntil,
//             onHalt flush, and the route's requireTerminalEvent checks)
//
// The request is a minimal harness request (one user message, no tools);
// provider/model/metaKey/compatibility come from the manifest so the two
// sides construct the same decoder initial state.
//
// Output: JSON array [{name, ok, events, error}] to --out.

const REF = process.env.DIFF_REF_ROOT
if (!REF) throw new Error("DIFF_REF_ROOT required")
process.on("unhandledRejection", () => {})

const { readFileSync, writeFileSync } = await import("node:fs")
const { Effect, Schema, Stream } = await import("effect")
const { Framing } = await import(`${REF}/packages/ai/src/route/framing.js`)
const { LLMRequest, Message, LanguageModel, ProviderID, ModelID } = await import(
  `${REF}/packages/ai/src/schema/index.js`
)

const args = process.argv.slice(2)
const get = (flag: string) => {
  const i = args.indexOf(flag)
  return i === -1 ? undefined : args[i + 1]
}
const manifestPath = get("--manifest")!
const outPath = get("--out")!

// -- family -> protocol module + carrier route (identity fields only) ------

const protocolFor: Record<string, () => Promise<any>> = {
  "anthropic-messages": async () => {
    const m = await import(`${REF}/packages/ai/src/protocols/anthropic-messages.js`)
    return { protocol: m.AnthropicMessages.protocol, route: m.AnthropicMessages.route }
  },
  "openai-chat": async () => {
    const m = await import(`${REF}/packages/ai/src/protocols/openai-chat.js`)
    return { protocol: m.OpenAIChat.protocol, route: m.OpenAIChat.route }
  },
  "openai-responses": async () => {
    const m = await import(`${REF}/packages/ai/src/protocols/openai-responses.js`)
    return { protocol: m.OpenAIResponses.protocol, route: m.OpenAIResponses.route }
  },
  "open-responses": async () => {
    const m = await import(`${REF}/packages/ai/src/protocols/open-responses.js`)
    const carrier = await import(`${REF}/packages/ai/src/protocols/openai-responses.js`)
    return { protocol: m.OpenResponses.protocol, route: carrier.OpenAIResponses.route }
  },
  "meta-responses": async () => {
    const m = await import(`${REF}/packages/ai/src/protocols/meta-responses.js`)
    const carrier = await import(`${REF}/packages/ai/src/protocols/openai-responses.js`)
    return { protocol: m.MetaResponses.protocol, route: carrier.OpenAIResponses.route }
  },
  "xai-responses": async () => {
    const m = await import(`${REF}/packages/ai/src/protocols/xai-responses.js`)
    const carrier = await import(`${REF}/packages/ai/src/protocols/openai-responses.js`)
    return { protocol: m.XAIResponses.protocol, route: carrier.OpenAIResponses.route }
  },
  "mistral-chat": async () => {
    const m = await import(`${REF}/packages/ai/src/protocols/mistral-chat.js`)
    return { protocol: m.MistralChat.protocol, route: m.MistralChat.route }
  },
  "cohere-chat": async () => {
    const m = await import(`${REF}/packages/ai/src/protocols/cohere-chat.js`)
    return { protocol: m.CohereChat.protocol, route: m.CohereChat.route }
  },
  gemini: async () => {
    const m = await import(`${REF}/packages/ai/src/protocols/gemini.js`)
    return { protocol: m.Gemini.protocol, route: m.Gemini.route }
  },
  "google-interactions": async () => {
    const m = await import(`${REF}/packages/ai/src/protocols/google-interactions.js`)
    return { protocol: m.GoogleInteractions.protocol, route: m.GoogleInteractions.route }
  },
}

const framingOf = (id: string) => {
  if (id === "sse") return Framing.sse
  if (id === "sse_done") return Framing.sseWithDone
  if (id === "sse_events") return anthropicMod.AnthropicMessages.framing
  throw new Error(`unknown framing ${id}`)
}

const anthropicMod = await import(`${REF}/packages/ai/src/protocols/anthropic-messages.js`)

const modules: Record<string, any> = {}
for (const family of Object.keys(protocolFor)) modules[family] = await protocolFor[family]()

// G-419/G-421: the venice-chat cassette group is a protocol WRAPPER over
// openai-chat (providers/venice.ts routes VeniceChat.protocol — marker
// stripping, encrypted-reasoning replay, conditional venice metadata). The
// manifest family is the shared carrier ("openai-chat"); select the real
// wrapper protocol for that group so both sides run the venice machine.
const veniceMod = await import(`${REF}/packages/ai/src/protocols/venice-chat.js`)

// -- decode one case --------------------------------------------------------

const isTerminalLLMEvent = (event: any) => event?.type === "finish" || event?.type === "provider-error"

async function decodeCase(caseSpec: any): Promise<any> {
  const { route } = modules[caseSpec.family]
  const protocol =
    caseSpec.family === "openai-chat" && caseSpec.group === "venice-chat"
      ? veniceMod.VeniceChat.protocol
      : modules[caseSpec.family].protocol
  const events: any[] = []
  let err: any = undefined
  if (caseSpec.status >= 400) {
    // Mirror the route pipeline: non-2xx fails transport status
    // classification before any framing/decode runs.
    return { name: caseSpec.name, ok: false, events, error: { message: `recorded status ${caseSpec.status}`, tag: "status" } }
  }
  try {
    const compatibility = caseSpec.compat
      ? new (await import(`${REF}/packages/ai/src/schema/index.js`)).LanguageModelCompatibility(caseSpec.compat)
      : undefined
    const model = LanguageModel.make({
      id: ModelID.make(caseSpec.model),
      provider: ProviderID.make(caseSpec.provider),
      route: route.with({ id: `diff-${caseSpec.family}`, provider: caseSpec.provider, providerMetadataKey: caseSpec.meta_key }),
      compatibility,
    })
    const request = new LLMRequest({ model, system: [], tools: [], messages: [Message.user("hi")] })

    const bytes = new TextEncoder().encode(readFileSync(caseSpec.body_file, "utf8"))
    const framing = framingOf(caseSpec.framing)
    const frames: string[] = Array.from(
      await Effect.runPromise(Stream.runCollect(framing.frame(Stream.fromIterable([bytes]) as any) as any)),
    )

    const decodeEvent = Schema.decodeUnknownEffect(protocol.stream.event)
    let state = protocol.stream.initial(request)
    let sawTerminal = false
    for (const frame of frames) {
      const event = await Effect.runPromise(decodeEvent(frame))
      if (sawTerminal) throw new Error("provider emitted an event after the terminal event")
      const [next, outputs] = await Effect.runPromise(protocol.stream.step(state, event))
      state = next
      for (const output of outputs) {
        events.push(output)
        if (isTerminalLLMEvent(output)) sawTerminal = true
      }
      if (protocol.stream.terminal?.(event)) break
    }
    if (!err && protocol.stream.onHalt) {
      const flushed = await Effect.runPromise(protocol.stream.onHalt(state))
      for (const output of flushed) {
        if (sawTerminal) throw new Error("provider emitted an event after the terminal event")
        events.push(output)
        if (isTerminalLLMEvent(output)) sawTerminal = true
      }
    }
    if (!sawTerminal) throw new Error("The provider response ended unexpectedly.")
  } catch (error: any) {
    err = { message: String(error?.message ?? error), tag: error?.reason?.constructor?.name ?? null }
  }
  return { name: caseSpec.name, ok: err === undefined, events, error: err ?? null }
}

const cases = JSON.parse(readFileSync(manifestPath, "utf8"))
const results: any[] = []
for (const caseSpec of cases) {
  let record
  try {
    record = await decodeCase(caseSpec)
  } catch (error: any) {
    record = { name: caseSpec.name, ok: false, events: [], error: { message: String(error?.message ?? error), tag: "driver" } }
  }
  results.push(record)
}
writeFileSync(outPath, JSON.stringify(results))
console.log(`ref driver: ${results.length} cases -> ${outPath}`)
