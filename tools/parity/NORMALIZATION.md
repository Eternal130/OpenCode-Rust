# Parity Normalization Spec (v2.0.24 reference vs Rust port)

This document defines **the canonical rewrite algorithm** for behavior-parity
comparisons. It is implemented exactly once, in `tools/parity/src/normalize.rs`
(`normalize_value` / `normalize_str`). Every payload compared by the parity
harness — HTTP bodies, SSE frames, goldens — passes through this function on
**both** sides before any diff. If you change this file, change the module (and
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
| Instance ids | string content pass (see §5.4) | `<SES_ID>`, `<EVT_ID>`, `<MSG_ID>`, `<USR_ID>`, `<FRM_ID>`, `<WRK_ID>`, `<ACP_ID>`, `<SH_ID>` |
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
| `time`, `timestamp`, `created`, `createdAt`, `updatedAt`, `updated`, `modified`, `startedAt`, `endedAt`, `expiresAt`, `lastUsedAt`, `date` | `<TS>` | `<ISO>` (only if the value matches the ISO regex §5.5) |
| `version` | — | `<VERSION>` (only if the value matches the version regex §5.6) |

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

Regex: `\b(evt|ses|msg|usr|frm|wrk|acp|sh)_[0-9A-Za-z]{8,}\b` →
`<UPPERCASEPREFIX_ID>` (e.g. `evt_11c556f9e001H4T7m94mksITaJ` → `<EVT_ID>`).

- Prefixes are verified against the v2.0.24 schema: `evt_` (event), `ses`
  (session), `msg_` (session message), `frm_` (form), `wrk` (workspace),
  `sh_` (shell); `usr_` / `acp_` are reserved for port-side namespaces.
- At least 8 identifier characters are required, so ordinary words never
  match. The reference id format is `prefix_` + 26 chars of `[0-9A-Za-z]`.
- **Identity is preserved within a comparison**: the same id always rewrites
  to the same token, so id *references* between fields still must agree.
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
normalize(value, env):
  object  → for each (key, value):
              number  → token(key) if key ∈ number-key tables, else number
              string  → keytoken(key, string) if applicable,
                        else content-passes(string, env)
              other   → normalize(value, env)          # recurse
  array   → [ normalize(item, env) … ]                 # order preserved
  string  → content-passes(string, env)                # bare strings (array
                                                       # elements, roots)
  scalar  → unchanged

content-passes(s, env):        # strictly in this order
  s ← replace each root in env (longest first) with "<SANDBOX>"
  s ← ORIGIN_URL.replace(s, "<URL>")                    # anchored, §5.2
  s ← EMBEDDED_URL.replace_all(s, "$1<HOST>:<PORT>")    # §5.3
  s ← ID_RE.replace_all(s, "<PREFIX_ID>")               # §5.4
  s ← ISO_RE.replace_all(s, "<ISO>")                    # §5.5
  return s
```

Order rationale: paths before URLs (a sandbox path inside a URL rewrites as a
path first), URLs before ids (ids never occur inside hosts/ports), ids before
timestamps (disjoint character classes; order kept for determinism).

## 7. Token vocabulary

`<SANDBOX>`, `<URL>`, `<HOST>`, `<PORT>`, `<PID>`, `<TS>`, `<ISO>`, `<MS>`,
`<VERSION>`, `<EVT_ID>`, `<SES_ID>`, `<MSG_ID>`, `<USR_ID>`, `<FRM_ID>`,
`<WRK_ID>`, `<ACP_ID>`, `<SH_ID>`.

Tokens are delimited `<`…`>` so they can never collide with real content
(JSON strings may not contain unescaped `<`-delimited tokens from this
closed set in practice; if one ever did, the golden diff would flag it).

## 8. Golden interaction

Goldens (`goldens/*.json`) always store **normalized** output. A golden
mismatch that survives normalization is a true behavioral divergence; run
with `PARITY_UPDATE=1` only after establishing the change is intentional,
and record the reason next to the golden update.
