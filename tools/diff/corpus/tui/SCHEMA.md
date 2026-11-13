# TUI diff scenario schema (`tui-diff-scenario/1`)

Scenario fixtures for channel 8 (TUI full-screen differential). One JSON file
per scenario in `tools/diff/corpus/tui/scenarios/`. Both implementers (the
bun-side reference harness and the Rust-side `oc-tui` harness) read the same
files; every field semantic below is normative for both.

## File format

```json
{"schema":"tui-diff-scenario/1","id":"home-empty","width":80,"height":24,
 "fetch":{"/api/path":{"status":200,"body":<json>}},
 "events":[<ref /api/event SSE payload 形状的 JSON 数组>],
 "keys":["<tui_diff.rs ScriptStep 键名>"],
 "settle_ms":200,"note":"一句话场景语义，双侧实现等价性论证处"}
```

### Field semantics

- `schema` — literal `"tui-diff-scenario/1"`.
- `id` — scenario identity; matches the file stem.
- `width` / `height` — the terminal grid size the harness renders at
  (both sides must render at exactly this size; size is part of the
  diff, cf. `tui_frame.rs`).
- `fetch` — HTTP stub map for the app's server endpoint. Keys are
  request paths (query-less); values are `{ "status": <int>, "body":
  <json> }`. The harness serves these for both sides so the app boots
  against identical data. Special case: `/api/event` has
  `body: { "sse": true, "initial": <event> }` — it is the SSE stream,
  which MUST open by emitting `initial` (the `server.connected` event,
  exactly like the reference fixture's `createEventStream().v2()`) and
  then replay `events` below in order.
- `events` — ordered array of `/api/event` SSE payloads, each the
  reference envelope shape `{ id, created, type, durable?, location?,
  data }` (see `/opt/opencode-ref/packages/tui/test/fixture/tui-client.ts`,
  `createEventStream`; event schemas live in the ref
  `packages/schema/src/session-event.ts` / `v1/session.ts`). Emitted as
  `data: <json>\n\n` frames after the initial `server.connected`.
- `keys` — scripted input tokens fed after boot + events, in order.
  Vocabulary is the `tui_diff.rs` `encode_key` table (below); empty
  array means the scenario is purely fetch/event-driven.
- `settle_ms` — settle budget after the last event/keypress before the
  frame capture; both sides use the same budget.
- `note` — one-sentence scenario semantics; the place where
  equivalence between the two implementations is argued.

### Session-route convention

Session scenarios (`session-*`) stub the session endpoints with
concrete ids (e.g. `/api/session/ses_smoke_text`). The harness boots
the app into that session: the session id is the one appearing in the
`/api/session` list stub plus `/api/session/active`.

## 端点普查 (Step 1 probe)

Probed by booting the reference v2.0.24 TUI through its own fixture
(`packages/tui/test/fixture/app.ts` `createAppFixture`, no keys fed,
default stubs) and recording every request path until settle. Probe
script (one-shot, not committed): `/tmp/opencode/probe-tui-endpoints/probe.ts`,
run with `bun --preload @opentui/solid/preload` from the reference
`packages/tui` (bun 1.3.10, the tui-dump invocation convention).

21 unique boot-time paths (all GET):

```
/api/agent
/api/command
/api/config
/api/event
/api/experimental/migration/v1
/api/form
/api/fs/list
/api/info
/api/integration
/api/location
/api/mcp
/api/mcp/resource
/api/model
/api/plugin
/api/project
/api/provider
/api/reference
/api/session/active
/api/shell
/api/skill
/api/vcs
```

Every scenario's `fetch` map covers all 21 census paths (bodies mirror
the reference fixture defaults in `tui-client.ts`). Session scenarios
additionally stub the session-route endpoints
(`/api/session`, `/api/session/<id>`, `/api/session/<id>/message`,
`<id>/inbox`, `<id>/permission`, `<id>/form`,
`/api/experimental/session/<id>/terminal`).

## 键名表 (key vocabulary)

Source of truth: `tools/parity/src/tui_diff.rs`, function
**`encode_key`** (module `tui_diff`, ~line 1206) — it maps each token
to its terminal byte sequence. `ScriptStep.keys` (same file, ~line
1089) is the deserialized shape these tokens come from.

| token | bytes |
|---|---|
| `enter` / `return` | `\r` |
| `esc` / `escape` | `\x1b` |
| `tab` | `\t` |
| `backspace` | `\x7f` |
| `delete` | `\x1b[3~` |
| `up` / `down` / `right` / `left` | `\x1b[A` / `\x1b[B` / `\x1b[C` / `\x1b[D` |
| `home` / `end` | `\x1b[H` / `\x1b[F` |
| `pageup` / `pagedown` | `\x1b[5~` / `\x1b[6~` |
| `space` | ` ` |
| `leader` | `\x18` (ctrl+x, the v2 default leader) |
| `alt+up` / `alt+down` / `alt+right` / `alt+left` | `\x1b[1;3A` / `\x1b[1;3B` / `\x1b[1;3C` / `\x1b[1;3D` |
| `ctrl+<a-z>` | `0x01..0x1a` (single ascii lowercase letter) |
| `raw:<literal>` | the literal's UTF-8 bytes |

Unknown tokens are a hard error on the Rust side; the bun side must
accept exactly the same set. The three smoke scenarios carry
`keys: []` (fetch/event-driven); later channel-8 scenarios use this
table.

## Event vocabulary used by the smoke corpus

All events are v2 durable events (ref `packages/schema/src/session-event.ts`):

- `session.updated` — `{ sessionID, info: SessionInfo }` (session bootstrap)
- `session.text.started` — `{ sessionID, assistantMessageID, ordinal }`
- `session.text.ended` — `{ sessionID, assistantMessageID, ordinal, text }`
  (the replayable full-value boundary for a text part)
- `session.step.ended` — `{ sessionID, assistantMessageID, finish, cost, tokens }`
