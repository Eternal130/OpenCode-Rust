# Parity Normalization Spec (v2.0.24 reference vs Rust port)

This document defines **the canonical rewrite algorithm** for behavior-parity
comparisons. It is implemented exactly once, in `tools/parity/src/normalize.rs`
(`normalize_value` / `normalize_str`; multi-part comparison units use the
`_with` variants with one shared `IdNumbering` run). Every payload compared by
the parity harness — HTTP bodies, SSE frames, goldens — passes through this
function on **both** sides before any diff. If you change this file, change
the module (and
vice versa); the unit tests in `normalize.rs` pin the behavior.

## 1. Purpose

Two runs of "the same" opencode server never produce byte-identical output:
instance ids, timestamps, ports, pids, temp paths, and jittered timings differ
by construction. Normalization collapses exactly those incidental values into
stable tokens so that everything **left over** must match exactly. The
normalizer is intentionally conservative: it only rewrites what is provably
run-specific, never content that could hide a real behavioral difference.

## 2. What gets rewritten

| Category | Trigger | Token |
| --- | --- | --- |
| Instance ids | string content pass (see §5.4) | `<SES_ID:n>`, `<EVT_ID:n>`, `<MSG_ID:n>`, `<USR_ID:n>`, `<FRM_ID:n>`, `<WRK_ID:n>`, `<ACP_ID:n>`, `<PER_ID:n>`, `<PSV_ID:n>`, `<CRED_ID:n>`, `<CON_ID:n>`, `<SH_ID:n>` |
| Timestamps (numeric, unix epoch) | key-driven (see §4) | `<TS>` |
| Timestamps (ISO-8601 strings) | key-driven, or anywhere in text | `<ISO>` |
| Jittered delays / durations (ms) | key-driven | `<MS>` |
| Ports | key-driven (`port`) | `<PORT>` |
| Ports inside URLs | string content pass | `<PORT>` (with `<HOST>`) or `<URL>` |
| Pids | key-driven (`pid`) | `<PID>` |
| Sandbox / tmpdir paths | string content pass (registered roots) | `<SANDBOX>` |
| Server base URLs | string content pass | `<URL>` (bare), `http://<HOST>:<PORT>` (embedded) |
| Version strings | key-driven (`version`) | `<VERSION>` |

### Why `version` is rewritten

`/api/info` reports the *build* version. The reference run via
`bun run --define OPENCODE_VERSION="2.0.24"` reports `2.0.24`, but a bare dev
run reports `local`; the Rust port reports its workspace version. All of
`v?X.Y.Z[-prerelease]`, `local`, `vlocal` collapse to `<VERSION>` so shape
parity is asserted independently of build channel. (A stricter same-version
assertion belongs in a dedicated non-normalized check, not here.)

## 3. What must match exactly (never rewritten)

- **Type / `_tag` / `type` names** — e.g. `"type": "server.connected"`,
  `"_tag": "UnauthorizedError"`. These are the contract.
- **Error codes / messages / HTTP status codes** — `"Authentication required"`
  must match byte-for-byte; `httpdiff` reports status divergence at
  `$status`.
- **Array ordering** — element order is preserved and significant. (An
  endpoint with intentionally unordered output gets an explicit sort in its
  own test, not a blanket normalization.)
- **Content** — model names, config values, message text, booleans, nulls.
- **Numbers** under any key not listed in §4 — an unexpected numeric drift is
  a divergence, not noise. Integer vs float is also distinguished (the JSON
  number `1` does not match `1.0`).

Object **key order** is not compared: `serde_json::Map` is a `BTreeMap` (the
workspace does not enable `preserve_order`), so both sides are key-sorted
deterministically before diffing.

## 4. Key-driven rewrites (objects only)

Applied when the value sits directly under a matching **exact, case-sensitive
object key**. Rewritten numbers become JSON *strings* — the type change is
intentional and part of the spec.

| Key(s) | Number → | String → |
| --- | --- | --- |
| `port` | `<PORT>` | — |
| `pid` | `<PID>` | — |
| `duration`, `elapsed`, `delay`, `jitter`, `timeout`, `took`, `interval` | `<MS>` | — |
| `time`, `timestamp`, `created`, `createdAt`, `updatedAt`, `updated`, `modified`, `startedAt`, `endedAt`, `expiresAt`, `lastUsedAt`, `date`, `started`, `completed`, `streamed`, `ran`, `idle`, `viewed`, `archived`, `at`, `from`, `to`, `next`, `expires`, `published`, `active` | `<TS>` | `<ISO>` (only if the value matches the ISO regex §5.5) |
| `version` | — | `<VERSION>` (only if the value matches the version regex §5.6) |

The timestamp key list is aligned to **every epoch-millisecond field in the
ported wire schema** (`crates/oc-schema/src`): the message `time` blocks
(`created`, `completed`, `streamed`, `ran` — `AssistantTime`/`ToolTime`),
retry scheduling (`at` — `AssistantRetry`/`session.retry.scheduled`,
`next` — `SessionStatus` retry), session watermarks (`idle`, `viewed`,
`archived` — `Session.Time`), `started` (`Shell.Time`,
`session.step.started`), stats windows (`from`/`to` — `Session.Stats.Range`),
integration attempts (`expires` — `AttemptTime`), credentials (`expires` —
`Credential.Value` oauth), projects (`active` — `Project.Time`), and
web-search results (`published`). The alignment is re-run whenever
`oc-schema` grows a millis field (the unit test
`every_ts_key_field_is_masked` enumerates this list).

Deliberately **not** treated as timestamps: `seq` (aggregate log position —
sequence comparison *orders by it*, masking would destroy the signal),
`version`, `ordinal`, `attempt` (counters), and `Prompt.Mention`
`start`/`end` (character offsets into prompt text, not wall-clock).

Keys not listed here are never key-rewritten; their values still get the
string-content passes below if they are strings.

## 5. String-content passes (applied to every string, in this order)

### 5.1 Sandbox roots (literal replace, longest root first)

Every root registered in `NormEnv` (the harness registers the reference
server's sandbox dir, e.g. `/tmp/opencode/parity-sandbox/ref-<pid>-<port>`)
is replaced literally by `<SANDBOX>`. Longest root wins, so nested roots do
not partially rewrite. Example:
`/tmp/opencode/parity-sandbox/ref-42-1234/tmp/opencode` →
`<SANDBOX>/tmp/opencode`.

### 5.2 Bare origin URL (whole-string match)

Regex: `^https?://(?:\[[0-9a-fA-F:]+\]|[A-Za-z0-9._~-]+)(?::[0-9]+)?/?$`
→ `<URL>`. Applies to values like `http://127.0.0.1:50713` (the `urls`
array in `/api/info`). Replaces the first (and only) match; because the
pattern is anchored, a whole-string match consumes the entire value.

### 5.3 Embedded URL authority (global)

Regex: `(https?://)(?:\[[0-9a-fA-F:]+\]|[A-Za-z0-9._~-]+)(:[0-9]+)` →
`${1}<HOST>:<PORT>`. Applies to URLs inside longer text (paths, messages):
`see http://127.0.0.1:50713/api/info` → `see http://<HOST>:<PORT>/api/info`.
An embedded URL **without** an explicit port is left untouched.

### 5.4 Instance ids (global)

Regex: `\b(evt|ses|msg|usr|frm|wrk|acp|sh|per|psv|cred|con)_[0-9A-Za-z]{8,}\b` →
`<UPPERCASEPREFIX_ID:n>` (e.g. `evt_11c556f9e001H4T7m94mksITaJ` → `<EVT_ID:1>`).

- Prefixes are verified against the v2.0.24 schema (`oc-schema/src/id.rs`):
  `evt_` (event), `ses` (session), `msg_` (session message), `frm_` (form),
  `wrk` (workspace), `per_` (permission), `psv_` (persistent-pty permission
  saved), `cred_` (credential), `con_` (integration attempt), `sh_` (shell);
  `usr_` / `acp_` are reserved for port-side namespaces.
- At least 8 identifier characters are required, so ordinary words never
  match. The reference id format is `prefix_` + 26 chars of `[0-9A-Za-z]`.
- **Numbering is first-seen canonical, per prefix, per normalization run**:
  within one run the first distinct `msg_…` becomes `<MSG_ID:1>`, the second
  `<MSG_ID:2>`, and so on; counters are independent per prefix. An id seen
  again reuses its number.
- A *normalization run* is one whole comparison unit on one side: one HTTP
  body, or one collected SSE sequence (`frames_to_json`), or one sequence
  group (§"Sequence comparison"). Both sides run the algorithm
  independently; their numberings agree exactly when corresponding ids are
  first-seen in corresponding structural positions (depth-first traversal;
  arrays in element order, object keys in sorted order).
- **Referential integrity is preserved and disagreement is detectable**: two
  positions referencing the same id get the same token, positions referencing
  *different* ids of the same prefix get *different* tokens — so an event
  pointing at the wrong message (same prefix, different id) survives
  normalization as a divergence. Collapsing all same-prefix ids to one token
  would hide exactly that class of bug and is forbidden.
- Known limits: first-seen numbering pairs ids structurally, not
  semantically — if two payloads each contain a *set* of same-prefix ids
  with no cross-references and no stable order, a consistent permutation of
  that set normalizes equal (permutation-invariance). Cross-references are
  what pin identities down. Sequence comparison scopes numbering per group
  (see below), so cross-*aggregate* references are checked by whole-body
  compares, not sequence compares.
- New prefixes are added here first (single source of truth), then to the
  regex in `normalize.rs`.

### 5.5 ISO-8601 timestamps (global)

Regex:
`\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})?` →
`<ISO>`. Timezone is optional (`toISOString` always emits `Z`, other emitters
may not).

### 5.6 Version strings (key-gated)

Regex: `^(?:v?\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?|v?local)$` — semver with
optional `v` and pre-release/build suffix, or a local dev marker. Applied
only under `version` keys; a non-matching `version` value (e.g.
`custom-build-thing`) stays literal.

## 6. The canonical algorithm (pseudocode)

```
normalize(value, env, ids):        # ids = one IdNumbering per run (§5.4)
  object  → for each (key, value):
              number  → token(key) if key ∈ number-key tables, else number
              string  → keytoken(key, string) if applicable,
                        else content-passes(string, env, ids)
              other   → normalize(value, env, ids)      # recurse
  array   → [ normalize(item, env, ids) … ]             # order preserved
  string  → content-passes(string, env, ids)                # bare strings (array
                                                       # elements, roots)
  scalar  → unchanged

content-passes(s, env, ids):      # strictly in this order
  s ← replace each root in env (longest first) with "<SANDBOX>"
  s ← ORIGIN_URL.replace(s, "<URL>")                    # anchored, §5.2
  s ← EMBEDDED_URL.replace_all(s, "$1<HOST>:<PORT>")    # §5.3
  s ← ID_RE.replace_all(s, "<PREFIX_ID:n>")             # §5.4, n from ids
  s ← ISO_RE.replace_all(s, "<ISO>")                    # §5.5
  return s
```

Order rationale: paths before URLs (a sandbox path inside a URL rewrites as a
path first), URLs before ids (ids never occur inside hosts/ports), ids before
timestamps (disjoint character classes; order kept for determinism).

## 7. Token vocabulary

`<SANDBOX>`, `<URL>`, `<HOST>`, `<PORT>`, `<PID>`, `<TS>`, `<ISO>`, `<MS>`,
`<VERSION>`, and id tokens `<PREFIX_ID:n>` where `PREFIX` ∈ {`EVT`, `SES`,
`MSG`, `USR`, `FRM`, `WRK`, `ACP`, `PER`, `PSV`, `CRED`, `CON`, `SH`} and
`n ≥ 1` is the first-seen number within the run (§5.4).

Tokens are delimited `<`…`>` so they can never collide with real content
(JSON strings may not contain unescaped `<`-delimited tokens from this
closed set in practice; if one ever did, the golden diff would flag it).

## 8. Sequence comparison (SSE event streams)

Frame-sequence parity (Phase 5+) is compared under the canonical policy
below, implemented by `sse::compare_sequences` and
`SseStream::collect_scenario`. Global SSE order across independent
aggregates is **nondeterministic** — two correct servers may interleave the
same per-aggregate events differently — so raw global order is never the
contract.

### 8.1 Grouping (per side)

Each **data** frame (heartbeats are transport noise, skipped) is assigned to
a group keyed by:

1. **`location`** — the envelope's `location` object (`directory`, optional
   `workspaceID`), plus
2. **the aggregate id**: `durable.aggregateID` (envelope durable events), or
   top-level `aggregateID` (`log.synced`), falling back to **`sessionID`
   inside `data`** (session events without a durable envelope).

Frames with neither location nor aggregate/session id (e.g.
`server.connected`) share one fixed `"global"` group. Group keys are
**diagnostics only** — raw ids differ per run, so groups are never paired by
key across sides.

### 8.2 Canonical order within a group

- If **every** frame of the group carries `seq`: ascending `seq`.
- Otherwise: arrival order (within one aggregate, arrival order is
  deterministic; only cross-aggregate interleaving is not). Mixed
  seq/no-seq groups therefore compare in arrival order.

### 8.3 Normalization scope

Each group is normalized as its **own id-numbering run** (§5.4), encounter
order = the group's canonical order. This makes id tokens invariant to
cross-aggregate interleaving. Consequently, *cross-aggregate* id references
cannot be checked by sequence comparison — they are checked by whole-body
compares, which use one numbering run per body.

### 8.4 Comparison

The two sides' **multisets of group orderings** are compared: every group
ordering (normalized canonical frame sequence) on one side must pair with an
equal ordering on the other side; counts matter (a missing or extra
aggregate is a divergence), group keys do not (pairing is by content).

**First-divergent-frame reporting**: the first unmatched group (reference
side first, in first-appearance order, then port side) is reported against
its best counterpart — the opposite-side leftover with the longest common
frame prefix — at the first frame index where the two orderings differ
(absent frames/groups render as `Null` on the missing side).

### 8.5 Collection alignment (N-frame scenarios)

Both sides collect with identical parameters via
`SseStream::collect_scenario(max_frames, quiescence, overall)`:

- stop at `max_frames` **data** frames (scenario watermark), or
- stop at **quiescence**: no data frame for `quiescence` (default
  `sse::DEFAULT_QUIESCENCE` = 2000 ms; heartbeats do not reset the window),
  or
- stop at `overall_timeout` (safety bound),

then compare per §8.4. A stream that ends (EOF) is quiescent by definition.
Fewer than `max_frames` frames may legitimately arrive; that is not an
error, the shortened sequence is simply compared.

## 9. Golden interaction

Goldens (`goldens/*.json`) always store **normalized** output. A golden
mismatch that survives normalization is a true behavioral divergence; run
with `PARITY_UPDATE=1` only after establishing the change is intentional,
and record the reason next to the golden update.
