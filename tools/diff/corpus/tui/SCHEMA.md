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
- `plugins` — OPTIONAL real-plugin payload list (B2 extension, user
  ruling 2026-11-14: the diff pin stays ref 2.0.24 and the payload is
  the user's REAL plugins; boot success/failure is decided by the
  fixture run, never by static analysis of the minified bundles).
  Shape: `[<payload-id>, ...]` with the symbolic ids of the pinned
  payload registry:
    - `"omo-slim"` — oh-my-opencode-slim 3.0.3 (npm entity under
      `/root/.cache/opencode/npm/oh-my-opencode-slim@latest/<stamp>/
      node_modules/oh-my-opencode-slim`; server entry
      `dist/server/index.js`, TUI companion export `dist/tui2.js`).
    - `"superpowers"` — superpowers 7.0.0 (git entity under
      `/root/.cache/opencode/npm/git-superpowers-*/<stamp>/node_modules/
      superpowers`; main `.opencode/plugins/superpowers.js`, pure skill
      system, no external deps).
    - `"broken"` — the DELIBERATELY-broken committed fixture
      (`tools/diff/corpus/tui/payloads/broken-plugin`, module top-level
      throw) for the plugin-fallback scenario: both sides boot it
      through the real seams and land in the failure state — the
      failure-STATE rendering is the diff face.
  **B2-fix ruling (2026-11-15, controller experiment)**: the first
  round's whole-screen ref boot failure was bun **1.3.10's
  `ResolveMessage` not being an `instanceof Error`** (it IS one under
  bun ≥ 1.4.2); ref `host.ts:17-43`'s swallow guard depends on it.
  Therefore the ref leg of every plugin case runs under
  **`TUI_DIFF_BUN`** (env, path to a bun executable) with version
  **≥ 1.4.2** — unset or older is a pointed infra error; non-plugin
  cases keep the environment bun (baseline no-drift). Under the gate
  BOTH real payloads boot on the ref side (probed: clean resting
  screens, omo-slim's real Solid sidebar renders at 320×80, and one
  RESIDUAL version-skew shape: loading omo-slim's tui entry degrades
  the ref's "/" slash-palette path — the CAUSES version-skew cluster
  owns such local degradation shapes).
  **Two staging roots**: the orchestrator stages each payload into
  `STAGE/plugins-ref/node_modules` (payload copies + hoisted deps +
  an `@opentui` symlink into the ref tree's host-provided renderer
  stack — the ref process runs the plugin's real Solid sidebar) and
  `STAGE/plugins-rust/node_modules` (same payload copies + deps, NO
  `@opentui` — the rust guest's `ensureOpentuiShims` writes its
  bounded bridge shims there; a real `@opentui` inside the guest
  breaks at `render()` with no renderer context, probed: "No renderer
  found"). For `omo-slim` the staged copy additionally carries a
  2-line root `tui.js` re-export shim: 2.0.24's local-dir loader
  resolves ONLY the `<dir>/tui.*` extension-probe convention
  (`Bun.resolveSync` never consults package.json exports for absolute
  specifiers), while the production package-name path
  (`packages.prepare`) is pinned away by the fixture's
  `prepare: async () => ({ directory: "" })` — the shim expresses the
  package's OWN declared `exports["./tui"]` entry in the one shape
  2.0.24 understands; the payload and the read-only cache entities
  are untouched. The manifest case carries BOTH lists: `plugins`
  (the ref-leg dirs, read by the ref driver) and `plugins_rust` (the
  rust-leg dirs, read by the rust driver).
  Driver semantics: the ref driver passes its dirs as the fixture's
  `config.plugins` (the plugin-keymap.test.tsx precedent; plugin
  scenarios insert a per-key-token idle — the palette search input
  mounts asynchronously and same-burst chars are dropped); the rust
  driver boots the real `PluginHost` + `TuiPluginRuntime` over the
  resolved entries (the plugin_ui_slot.rs assembly) and surfaces boot
  failures (chip + toast) via `surface_boot_failures`.
  **plugins_unavailable skip semantics**: a driver that cannot attempt
  the boot for ENVIRONMENT reasons (no bun/node guest runtime, staged
  payload missing) emits `plugins_unavailable: true` on its grid. The
  orchestrator rules consistency: both sides marked ⇒ the case is
  skipped (dropped from the gate, listed in the report); exactly one
  side ⇒ infra failure (exit 1). A plugin that boots and FAILS is NOT
  unavailable — the failure state is ordinary diff surface (see
  `plugin-fallback` + the G-425 registration).
- `anim` — OPTIONAL frame-sequence mode (`tui-diff-scenario/1` A2
  extension). Shape `{"ticks": K, "tick_events": [[<event>...], ...]?}`.
  Absent (every pre-A2 scenario) = pure resting mode; the resting
  60-case semantics are unchanged. When present, the drivers — after
  the ordinary resting settle/capture — additionally capture K+1 full
  grids (`frames`, SCHEMA A2): frame 0 is the resting frame.
  - `ticks` K: frames are captured at sequence positions k = 0..K.
  - **`ANIM_TICK_MS` = 100** (constant, normative for the rust side;
    aligned with the rust live pump's 100 ms tick). The rust driver
    steps its virtual clock to k·ANIM_TICK_MS before each capture
    (`store.advance_clock` + `toast_advance`; the render is a pure
    function of (store state, clock) — A1).
  - `tick_events` — optional per-tick event injection: entry k (an
    array of `/api/event` payloads, default `[]`) is emitted on the
    SSE stream right before tick k's capture (k = 1..K; entry 0 is
    ignored — the resting settle owns tick 0). This is how streaming
    delta scenarios feed one delta per tick.
  - **Ref-side comparability boundary (probed, A2)**: injecting the
    renderer `ManualClock` (`@opentui/core/testing`) into the FULL app
    is natively unstable — boot deadlocks on a frozen clock (boot
    timers ride the renderer clock), and with a real-time-driven
    ManualClock every post-event capture crashes in native buffer
    reads (segfault `bufferGetCharPtr`/`getRealCharBytes`; probe log
    in the a2 report). The ref driver therefore boots the STOCK
    fixture (`animations: false`, system clock) and its frames are
    EVENT-driven (tick_events + settle + capture, no clock steps).
    Clock-driven rust animation (spinner glyph rotation, caret blink,
    knight spinner) vs the ref's animation-gated static shapes is a
    REGISTERED harness artifact family (CAUSES
    "A2 ref 动画门控夹具伪影"), not a silent pass: the frames
    dimension still asserts every event-driven content change
    exactly, and a future stable ref clock slots into the same
    schema. Ref-side non-clock-injectable animation surfaces (toast
    dismissal on a REAL global setTimeout — ui/toast.tsx:142; caret
    blink via native cursor style) remain out of frame-sequence scope.
- `note` — one-sentence scenario semantics; the place where
  equivalence between the two implementations is argued.

### 尺寸档位 (size tiers)

The corpus renders every scenario at **three tiers by default**:
`80×24`, `120×30`, and `320×80`. A scenario file carries a single
`width`/`height` pair (the **primary tier** — the file's own declared
size, `80×24` for every current scenario); the orchestrator/harness
derives the other two tiers (`120×30`, `320×80` — the **mirrored
tiers**) by mirroring — same schema, id, fetch stubs, events, keys and
settle budget, only the grid size changes. Scenario ids stay
un-suffixed for the primary tier; harnesses report mirrored tiers as
`<id>@120x30` / `<id>@320x80`. The 320×80 tier is the 4K full-screen
stress tier (the operator's real terminal is 237×54; 320×80 exceeds it)
and is what pushes the session frame into the sidebar auto-dock regime
(ref `session-frame.tsx` `wide()`): `width − verticalTabsWidth > 120`
auto-shows the sidebar under the default `session.sidebar = "auto"`
config (child sessions excepted).

**`tiers` override array** (reserved; no v1 scenario uses it): a
scenario MAY declare `"tiers": [[w,h], ...]` to run **exactly that
subset of tiers instead of the default three** (each entry `[w,h]`;
the un-suffixed case id is reserved for the entry matching the file's
own `width`/`height`, every other entry gets the `@WxH` suffix). This
exists for scenarios where an extra tier is meaningless (e.g. a modal
that cannot reflow) or genuinely broken-by-design at some size — and
**must be justified in the scenario's `note`**. Adjudication review
(GAPS rounds) treats an unjustified `tiers` override as green-gaming
the gate: it is not a license to drop a size that merely diverges.

### 主题 (theme pinning)

Channel 8 renders every scenario under the reference default theme: the
built-in `opencode` **v2 theme in dark mode**
(`packages/tui/src/theme/assets/v2/opencode.json` in the ref tree) — the same
corpus theme as the tui-dump goldens (`tools/parity/tui-dump/README.md`,
`theme: { id: "opencode", label: "v2-dark", mode: "dark" }`). Both sides must
resolve and mount that exact theme document; no scenario-level theme override
exists in `tui-diff-scenario/1`.

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
the reference fixture defaults in `tui-client.ts`; note `/api/config`
is a **bare array** `[]` — not the `{location, data}` envelope — and
`/api/form`'s location omits `project.canonical`, both exactly as the
fixture returns them). Session scenarios additionally stub the
session-route endpoints (`/api/session`, `/api/session/<id>`,
`/api/session/<id>/message`, `<id>/inbox`, `<id>/permission`,
`<id>/form`, `/api/experimental/session/<id>/terminal`).

**stub_gap rule**: any endpoint the TUI actually requests that a
scenario's `fetch` map does not cover is a `stub_gap` and counts as an
**infra failure** for that run (never a render diff, never a silent
503/fallback). Harnesses (T2 bun-side, T4 rust-side) must detect and
report stub_gaps before comparing frames.

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
