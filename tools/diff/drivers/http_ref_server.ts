// Ref-side boot for the HTTP full differential (diff channel 6).
//
// Boots the real @opencode/server application graph via ServerFetch.make —
// the documented embedder entry (`packages/server/src/fetch.ts`: "the entry
// for runtimes that hand requests to the embedder instead of letting it
// listen: ... Bun.serve") — and fronts it with Bun.serve on an ephemeral
// loopback port. Staged to /tmp by tools/diff/http_full.py with a
// node_modules tree of symlinks into the read-only ref checkout, so the
// ref repo's git tree is never written. All durable roots (HOME, XDG_*,
// TMPDIR, OPENCODE_TEST_HOME) are pointed at /tmp by the orchestrator.
//
// stdout contract: one line `DIFF_READY <port>` once listening. Anything
// else on stdout is application logging; the orchestrator scans for the
// marker. Dies with SIGTERM/SIGKILL from the orchestrator (no repo state).

import { Effect, Scope } from "effect"
import { ServerFetch } from "@opencode/server/fetch"

const options = {
  app: { version: process.env.DIFF_APP_VERSION ?? "test-version" },
  database: { path: ":memory:" },
  config: { project: false },
  models: { fetch: false },
  fs: { filewatcher: false },
} as const

// The application layer builds EAGERLY inside the caller's scope
// (fetch.ts: do not convert to lazy first-request build). The scope is
// intentionally kept open for the process lifetime.
const scope = await Effect.runPromise(Scope.make())
const handler = await Effect.runPromise(
  Effect.provideService(ServerFetch.make(options as any), Scope.Scope, scope),
)

const server = Bun.serve({
  hostname: "127.0.0.1",
  port: 0,
  fetch: (request) => handler(request),
})

console.log(`DIFF_READY ${server.port}`)
