# D-03 spike — plugin hook mutation semantics (copy-in/copy-out vs v2 by-reference)

**Verdict: copy-in/copy-out CAN reach observable parity for the entire hook
surface, at near-zero runtime cost.** Parity is a property of *call-site
discipline + apply-point timing*, not of shared memory. v2 itself never reads
a hook event mid-dispatch — every invocation site is trigger-then-consume —
so the only thing a subprocess boundary cannot reproduce is behavior v2
plugins were never promised (held references past return, non-JSON stash
values, swapping host-internal JS objects). One v2 call site (`session.retry`)
discards the trigger's return and reads its by-reference-mutated local — and
our port had silently dropped those mutations; that is now fixed, along with
three other apply-point/timing deltas. Remaining divergences are documented
as permanent, each traded against crash isolation.

## 1. The v2 oracle (what by-reference actually buys)

`PluginHooks.trigger` (packages/core/src/plugin/hooks.ts:88-95) walks
callback entries in **registration order**, handing each the **same object**,
awaiting each before the next:

```ts
for (const entry of callbacks.get(key(domain, name)) ?? []) {
  if (entry.options?.providerID !== undefined && entry.options.providerID !== eventProviderID(event)) continue
  yield* entry.callback(event)
}
return event
```

Exhaustive audit of every v2 invocation site (permission.ts:179, tool.ts:104/
137/149, shell.ts:274, model-request.ts:305/343/355/375/383/387/399/422/428,
prompt.ts:40):

* **No host read of the event while handlers run.** JS is single-threaded and
  every site is `const x = yield* hooks.trigger(...)` then reads `x.field`.
  The host can only observe mutations *after* the dispatch resolves.
* **The one mid-chain host read is the provider-scope check itself**
  (`eventProviderID(event)` per entry, against the live mutated event).
* **One call site relies on by-reference past a discarded return**:
  `session.retry`. model-request.ts:399 wraps the trigger with
  `.pipe(Effect.asVoid)` — return discarded — and retry.ts:80-87 then reads
  its *local* `event.decision`. Plugin mutations reach the caller only
  through the shared reference.
* Within one `trigger`, plugin B's callback sees plugin A's mutations (same
  object) — cross-plugin visibility *inside one dispatch*.

## 2. Our architecture, mapped

* In-process: `oc-core::HookRegistry` dispatch chains `&mut` through every
  handler and returns the final value — true v2 semantics for native
  handlers.
* Guest plugins (bun subprocess): `Bridge::dispatch_hook(plugin_id, …)` does
  one `hook.dispatch` RPC per **plugin**; the guest (`host.js
  handleHookDispatch`) hands that plugin's handlers the same JS object
  sequentially and returns the mutated event; the host chains plugin→plugin
  (reference loop: `tests/common/mod.rs:105`). Cross-plugin mutation
  visibility is proven e2e by `corpus_hook_mutation_ordering`
  ("original text [mutated-by-A] [mutated-by-B]").
* **The engine-side adapter (guest hooks → oc-core `HookRegistry`) does not
  exist yet.** The bridge, the reference chain loop, and registration
  coverage (`PluginHook`, `hook_subscribers`) all exist; wiring them into the
  engine's dispatch/`has()` is future work this spike specs (§5).

## 3. Per-hook table

Classes: **(a)** parity via return-value application (no work) · **(b)**
parity requires matching v2's *timing/apply-points* (fixed where it mattered)
· **(c)** inherently impossible across the boundary → permanent registered
divergence.

| hook | v2 semantics | our semantics (post-spike) | observable delta | class |
|---|---|---|---|---|
| `session.prompt` | shared obj; `event.prompt` consumed after (prompt.ts) | typed event, `&mut` chain, consumed after | none (production admission path not wired yet — adapter TODO) | (a) |
| `session.context` / `compaction` / `generate` | shared obj; `shaped` system/messages/options/tools consumed after; compaction `result` short-circuit mutated in place | dispatch → `shaped` consumed after (model_request.rs) | none | (a) |
| `session.title` | shared obj; `request` + `result` short-circuit | dispatch → consumed after | none | (a) |
| `session.model.request` | shared obj; `modelHook.baseURL/headers` consumed after | dispatch → consumed after | none | (a) |
| `session.retry` | **return discarded**; caller reads local by-ref (retry.ts:80-87) | was: `dispatch(event.clone())` result dropped ⇒ plugin decision overrides silently ignored — **now writes the dispatched event back through `&mut`** | fixed + regression test; flagship-visible (see §4) | **(b) FIXED** |
| `session.http.request` / `http.response` | trigger inside the fetch wrap; request/response consumed after (body re-read via `clone().arrayBuffer()`) | typed in hooks.rs; runner wrap unwired yet | none once wired (apply-after model fits) | (a) |
| `session.experimental.ws.*` | trigger → `event.field` mapped | typed; unwired | none once wired | (a) |
| `tool.execute.before` | shared obj; `before.input` consumed after; rejection = typed failure | dispatch → `before.input`; `HookError::ToolRejected` | none | (a) |
| `tool.execute.after` (completed) | content/output/metadata read after; `normalizeContent` **applies emptied content** (runtime.ts:259 → stringified-output fallback) | was: emptied content kept the original output — **now applies emptied content with the output fallback** | fixed + test | **(b) FIXED** |
| `tool.execute.after` (error leg) | `return yield* afterEvent.error` — plugin rewrite becomes the failure (tool.ts:140) | was: dispatched event discarded, original failure returned — **now applies message/metadata** (variant classification Error/PermissionRejected kept — see residuals) | fixed + test | **(b) FIXED** |
| `permission.evaluate` | shared obj; `event.effect/message` consumed after | typed; production dispatch site unwired | none once wired | (a) |
| `shell.create.before` | shared obj; `invocation` fields consumed after | typed; unwired | none once wired | (a) |
| `aisdk.sdk` / `aisdk.language` | plugin swaps the **in-process AI-SDK package / LanguageModel object** the host then uses | typed Rust events; a JS object cannot cross the process boundary, and the host's model path is Rust (`oc-ai`) | permanent divergence (§6) | **(c)** |
| provider-scoped entries (all session/aisdk points) | scope re-checked **per entry against the live event** — mid-chain provider rewrites re-scope later entries | was: provider snapshotted before the loop — **now re-read per entry** (hooks.rs) | fixed + test | **(b) FIXED** |
| dispatch ordering (all points) | per-**callback** registration order | was: alphabetical by plugin id (`BTreeSet`) — **now registration order** (per-plugin granularity; see residual R3) | fixed + e2e test order preserved | **(b) FIXED** |
| handler failure mid-chain | dispatch **fails**; later handlers skipped; caller aborts (yield* propagates) | guest was: errors collected, chain continued, `ok:false` ignored — **now the first failure rejects the RPC** (host.js) and surfaces as `HostError::Guest` | fixed + e2e test (`corpus_hook_failure_fails_the_dispatch`) | **(b) FIXED** |

## 4. The flagship probe (is (b) real? — yes, once, decisively)

oh-my-opencode-slim 3.0.3 (staged under `/root/.cache/opencode-rust/npm`)
registers `session.{prompt,context,model.request,compaction,retry}` and
`tool.execute.{before,after}` — one registration per point (no interleaved
multi-registration ⇒ residual R3 is theoretical for it). All its bridges are
plain property mutation (`event.system.push`, `event.headers[...]`,
`applyRuntimeProfileOptions(event, …)`) — copy-out compatible. Except one:

* **`session.retry` — foreground-fallback** (dist/index.js:39883-39901): on a
  retryable error it switches models via `ctx.session.switchModel`, then sets
  `event.decision = { retry: true, delay: this.retryDelayMs }` — mutating the
  shared object the caller reads after the hook. With our pre-spike
  clone-and-discard, the flagship's retry override (its headline feature)
  would silently no-op. This is the concrete case that made class (b) real;
  it is fixed and covered by
  `runner_retry_prepared_hook_writes_mutations_back`.

No flagship (or v2) code reads a hook event concurrently with handlers, and
no plugin holds event references past return — the (c) class is empty in
practice for it.

## 5. Recommendation

**Close-by-timing-fixes (done in this spike):**

1. `model_request.rs` retry closure writes the dispatched event back through
   the `&mut` (the flagship-visible fix).
2. `hooks.rs` dispatch re-reads `hook_provider()` per entry (v2 per-entry
   scoping against the live event).
3. `tool/registry.rs`: error-leg `execute.after` rewrites apply to the tool
   failure; completed-leg emptied content applies (stringified-output
   fallback).
4. `registrations.rs`/`supervisor.rs`: hook subscriptions keep registration
   order (`Vec`, dedup on insert) — dispatch order no longer alphabetical.
5. `host.js handleHookDispatch`: first handler failure fails the RPC
   (v2 fail-the-dispatch), instead of swallow-and-continue.

**Engine-adapter spec (the future lane; this spike's binding guidance):**
register one `HookHandler` per (guest plugin, hook point) into the location's
`HookRegistry` at snapshot-settle time, whose `call(&mut event)` does
`bridge.dispatch_hook(plugin, domain, name, to_json(event))` and on Ok
writes the result back field-wise (`*event = from_json(...)`). Chaining,
ordering, provider scoping, and `has()` then come from the registry for free
— the reference loop in `tests/common/mod.rs` is exactly this shape. Guest
`tool.execute.before` rejections need a recognizable wire shape (e.g.
`{toolError: {...}}` in the RPC error) to map to `HookError::ToolRejected`.
Timing rule for the adapter: **apply at v2's apply-points, never earlier** —
which the registry loop already guarantees.

**Permanent divergences (register, with rationale):**

* **R1 `aisdk.sdk`/`aisdk.language`** — the hook's payload *is* a host-internal
  JS object; a Rust model path can never consume a guest-supplied
  `LanguageModelV3`. Equivalent power arrives via data-shaped surfaces
  (route/options swaps in `session.model.request`, which we support). Benefit
  retained: the guest can never hold host-internal objects.
* **R2 post-return mutation of a held reference** — a plugin capturing the
  event object and mutating it after its handler resolves cannot affect the
  host (the copy already crossed back). In v2 such a late write lands on the
  live object and can race later handlers. Pathological by definition (the
  plugin observes its own promise having resolved); isolation benefit: hook
  results are immutable snapshots once applied.
* **R3 per-callback vs per-plugin interleaving** — if plugin A registers the
  same hook point twice *interleaved* with plugin B's registration, v2 fires
  A1,B,A2; we fire A1,A2,B (one RPC per plugin). Matching exactly would need
  per-callback RPCs (handler tokens on `hook.dispatch`) — not worth the RPC
  inflation; the flagship doesn't multi-register.
* **R4 non-JSON stash values** — hook events are JSON contracts in v2
  practice; a plugin stashing a `Date`/class instance on the event loses it
  in our JSON round-trip (v2 passes it through to later handlers).
* **R5 retry-hook failure channel** — a *failing* retry hook in v2 fails the
  step (the effect dies); ours logs-and-continues with the host decision
  (`PreparedRetryHook` returns `()` — changing the signature is engine-lane
  scope, noted for the adapter spec).

**Residual nits (documented, deliberately not changed):** error-leg variant
re-classification (a plugin cannot turn a `PermissionRejected` into a plain
`Error` — v2 has one error shape; ours keeps classification and applies
message/metadata); `metadata: null` from a plugin keeps original metadata
(only an object replaces it).

## 6. Work performed

Code (all gated, tests included — see §7):

* `oc-core/src/hooks.rs` — per-entry provider re-read; new test
  `hooks_provider_scoping_rereads_the_live_event_per_entry`.
* `oc-core/src/session/runner/model_request.rs` — retry write-back; new test
  `runner_retry_prepared_hook_writes_mutations_back` (tests_misc.rs).
* `oc-core/src/tool/registry.rs` — after-hook application on both legs; new
  tests `tool_pipeline_after_hook_error_rewrite_shapes_failure`,
  `tool_pipeline_after_hook_can_suppress_content` (tool/tests.rs).
* `oc-plugin-host/src/registrations.rs` + `supervisor.rs` — registration-order
  hook subscriptions (`add_hook`).
* `oc-plugin-host/src/context/mod.rs` — `hook_subscribers` first-occurrence
  order, non-adjacent dedup.
* `oc-plugin-host/src/host/host.js` — fail-the-dispatch on handler error.
* `oc-plugin-host/tests/fixtures/hookfail.ts` + corpus test
  `corpus_hook_failure_fails_the_dispatch`.

## 7. Verification

* `cargo test -p oc-core --lib`: **740 passed, 0 failed** (baseline 736 + 4
  new).
* `cargo test -p oc-plugin-host`: **all green** (incl. bun-spawning corpus,
  flagship, storm/fuzz suites; 4-test corpus incl. the new failure test).
* `cargo check --workspace --all-targets`: clean, no warnings.
* `cargo clippy -p oc-core -p oc-plugin-host --all-targets`: clean.
* Pre-existing, unrelated: `oc-server` `handlers_integration` full-suite run
  fails `handlers_shell_round_trip` + `handlers_vcs_init_and_status` from
  cross-test interference — **reproduced on unmodified `main`**; each passes
  in isolation with this lane's changes applied.

## 8. Conclusion landing (lane P3-d03, 2026-10-16)

The ROADMAP disposition is closed: **resolved — equivalent semantics +
permanent registrations.** What this lane added on top of §5-6:

* `corpus_hook_mutation_replacement_shapes`
  (oc-plugin-host/tests/corpus.rs; fixtures `hookswap-a.ts`/`hookswap-b.ts`)
  — the first e2e proof that the *shapes* of v2 mutation, not just nested
  appends, cross the bridge host-observable: whole-field replacement (the
  `event.input = repair(...)` pattern, tool-input-repair.ts:33), scalar
  rewrite, and key deletion all reach the host consumer AND the next plugin
  in the chain, through the real bun subprocess + NDJSON boundary.
* The registered divergences asserted as documented behavior in the same
  test: R2 (a post-return `setTimeout` mutation on the held event is
  provably invisible to the host and later plugins, even after the timer
  fired) and R4 (a stashed `Date` crosses as its JSON string coercion —
  later plugins observe `typeof === "string"`).
* ROADMAP.md §3 D-03 row rewritten to the resolved disposition with the
  citations above.

Net D-03 position: every v2-observable hook-mutation behavior is achieved
by copy-in/copy-out applied at v2's apply-points (the exhaustive §1 audit
is the proof of coverage); what remains divergent is precisely the set a
process boundary buys isolation from (R1-R5) — registered with the
isolation-benefit rationale rather than papered over.

---

# SPEC-R6 — E12 search/filter first-pass findings (session-list surface)

Corpus `tools/parity/spec-canaries/e12_search_corpus.json` (I9: fixture-derived
synthetic); runner `crates/oc-tui/tests/spec_e12_first_pass.rs` (ours side =
REAL `filter_indices` code path; ref side = 1:1 port of
`tui/src/component/dialog-session-list.tsx:86-99`, cross-checked against the
hand-computed corpus cells). 12 queries → **9 divergent, 3 parity**:

1. **Child-session leakage** (E12-F1): ours lists forked children
   (`ses_child`) as filter hits; ref excludes `parentID != null` whenever a
   query is active (`dialog-session-list.tsx:97`). Ours shows them at all
   times (history-based options).
2. **Accent equivalence ours-only** (E12-F2): query `café` matches
   `CAFE overhaul` and `cafe` matches `café menu parser` in ours (fuzzy
   equivalence); ref `includes()` is byte-exact after case-fold — no match.
3. **NFD normalization** (E12-F3): NFD query `cafe + U+0301` matches NFC
   titles in ours; ref = no match (no normalization either side, but ours'
   fuzzy collapses the difference). Normalization audit: ref = case-fold
   only (toLowerCase), no NFC/NFD; ours = case-fold + fuzzy equivalence.
4. **Substring vs fuzzy semantics on session list** (E12-F4):
   `loginredirect` (concatenated) and `zln` (abbreviation) match in ours,
   not in ref. Root cause: ours renders `session.list` through the shared
   `dialog_select` fuzzy picker (`command_palette.rs:557-577`); ref has a
   DEDICATED session list with plain substring filter + server-search
   fallback.
5. **Whitespace** (E12-F5): doubled-space titles collapse for ours
   (`wide spaces` matches ` Wide  spaces  title `); ref byte-exact → no
   match.
6. **Empty/whitespace query** (E12-F6): ours returns NOTHING for
   whitespace-only query; ref trims → empty → returns ALL sessions
   (children included — parentID exclusion only applies with an active
   query).

Disposition: F4 is a design decision point (which semantics does the port
want for the session-list surface?); F1/F6 are behavioral bugs candidates;
F2/F3/F5 are semantics to ratify. All six need the LIVE ref run to confirm
the port (blocked this round — ref runtime copy lacks deps).

# SPEC-R6 — E5 inherited-finding (contrast)

Default dark theme (committed `assets/v2/opencode.json` through the resolver
chain): `border.base` on `background.base` measures < 3:1 (UI threshold).
Inherited from the ref theme (same source document) — listed, dispositioned,
not parity-blocking per the v5 E5 clause; ours must be ≥ ref ratio per cell
(holds — same values).

# SPEC-R6 — E2 note

StartupLoading gate-behavior assert (E2-a: stretch boot under vt-clock,
indicator appears at 500ms / escalates at 3s) is PENDING a boot-stretch
injection harness; our measured boot reaches interactive at ~569ms median —
just past the 500ms show threshold, so the gate is live-reachable.
