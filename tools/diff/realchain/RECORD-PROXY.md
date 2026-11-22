# RECORD-PROXY — opencode v2 daily-traffic recording proxy

`tools/diff/realchain/record_proxy.py` is a pure-Python-3.12-stdlib
HTTP reverse proxy: it forwards opencode v2's (the official TS
release, 2.2.x) day-to-day provider wire traffic (tool calls, file
edits, prompts — all real interactions) verbatim to the real upstream,
while recording every interaction as a **cassette v2** file,
automatically accumulating a corpus for the channel-9 replay gateway
(replay_gateway.py, another lane) to replay in the original timing
later.

## Enablement in three steps

**Step 1 — prepare routes.json** (you can copy the sample
`tools/diff/realchain/routes.sample.json` directly):

```json
{
  "anthropic": "https://api.anthropic.com",
  "openai": "https://api.openai.com"
}
```

**Step 2 — start the proxy** (keep it running in a terminal, or inside
tmux/screen):

```bash
python3 tools/diff/realchain/record_proxy.py \
    --routes tools/diff/realchain/routes.sample.json \
    --corpus-dir ~/oc-corpus \
    --port 8099
```

On startup it prints the listen address and the routing table. The
corpus directory is created automatically.

**Step 3 — change each provider's baseURL in the opencode v2 config**.
Edit `~/.config/opencode/opencode.json`, replacing each provider's
`options.baseURL` with the local proxy address; the route name
corresponds to the key in routes.json:

| provider original baseURL | change to |
|---|---|
| `https://api.anthropic.com` | `http://127.0.0.1:8099/r/anthropic` |
| `https://api.openai.com/v1` (keep AI-SDK-style URLs that already append `/v1` as-is, see below) | `http://127.0.0.1:8099/r/openai/v1` |

Rule: the local URL looks like
`http://127.0.0.1:PORT/r/<route>/<rest>`, and the proxy maps it
verbatim to `<the route's base URL from routes.json>/<rest>`. So you
only replace the official domain part with `/r/<route>` and leave the
path suffix untouched.

## Hot-reload verification conclusion (measured, 2026-11-17)

**Conclusion: opencode v2.2.7's config watcher really does hot-reload
baseURL — after editing the config no restart is needed; the next
conversation goes through the new address.**

Experiment method (stub upstreams, no real provider touched):

1. Start two local stub upstreams (ports A=4721, B=4722, emulating two
   different baseURLs, logging every hit);
2. In an isolated XDG directory write `opencode.json`
   (`@ai-sdk/openai-compatible` provider, baseURL→A), start
   `opencode serve` (v2.2.7, ~/.opencode/bin/opencode);
3. Via the HTTP API (`POST /api/session` + `POST
   /api/session/{id}/prompt`, Basic auth) send the first message →
   the stub log hits port A (2 hits: title generation + main reply,
   both on A);
4. With the server process **still running**, rewrite baseURL→B in
   `opencode.json`; wait 3 seconds and send the second message → all
   hits land on port B (zero hits on A).

Evidence (stub hit logs, Unix timestamps): after prompt A,
`[{"port":4721,"path":"/v1/chat/completions",...},{"port":4721,...}]`;
after the config change, prompt B →
`[{"port":4722,...},{"port":4722,...}]`.
Full experiment log: `.test-logs/hotload-experiment.log` (reproduction
script sketch above; API auth is `Authorization: Basic
base64("opencode:<server password>")`, the server password appears in
the `opencode serve` startup log).

Therefore **daily onboarding needs no opencode restart**: after
changing baseURL, the next new session (or the next message in an
ongoing session) is recorded through the proxy. If your version
behaves differently, the fallback path is: quit opencode → change
baseURL → `opencode` (inside the TUI `/resume`, or `opencode -c` to
continue the session).

## Privacy notes

* **Key red line**: `Authorization` / `x-api-key` / `api-key` (and
  cookie) headers are passed through to the upstream verbatim (your
  usage is unaffected) but are **never written to disk** — in the
  cassette these headers are always recorded as `<redacted>`. The
  self-test explicitly asserts that key bytes appear in no on-disk
  file.
* **Content stays local**: cassettes are written only to the local
  directory you chose with `--corpus-dir`; the proxy does no network
  reporting. Note: a cassette **contains the full request/response
  plaintext** (that is its purpose) — tighten the directory
  permissions yourself (e.g. `chmod 700`).
* **Crash safety**: every interaction is atomically written on
  completion (tmp+rename); a proxy crash/restart never damages
  existing cassettes.
* **Later ingestion process (planned)**: once the corpus has
  accumulated, the established age workflow applies — local redaction
  review → age encryption → packaging into the replay corpus;
  nothing enters any repository unencrypted. Until then the
  corpus-dir is purely private data on your machine.

## Cassette v2 format (pinned spec)

```json
{"version": 2,
 "metadata": {"recordedAt": "...", "proxy": "record_proxy/1",
              "route": "anthropic", "model": "claude-...",
              "transport": "http"},
 "interactions": [{"transport": "http",
   "request": {"method": "POST", "path": "/v1/messages",
               "headers_redacted": {"x-api-key": "<redacted>"},
               "body": "..."},
   "response": {"status": 200, "headers_redacted": {},
                "frames": [{"blob": "event: message_start\n...",
                            "delay_ms": 342.1}]}}]}
```

`frames` records per SSE event chunk; `delay_ms` is the milliseconds
elapsed since the previous chunk (the first chunk is the TTFT);
non-streaming JSON responses collapse to a single frame. When the W5
replay gateway merges, frames+delays reconstruct the TTFT/TPS timing.

## Passthrough fidelity

* SSE forwarded chunk by chunk: flushed to the client on the
  upstream's first byte — zero added TTFT latency;
* Status codes and headers passed through faithfully (only hop-by-hop
  headers stripped; forwarding uses chunked encoding or an exact
  Content-Length);
* Upstream HTTPS via the Python standard-library TLS;
* Concurrent-session safety: one thread per connection, cassette
  writes serialized.

## Self-test

```bash
python3 tools/diff/realchain/record_proxy_test.py
```

4 cases (stub upstreams, no real provider touched): SSE byte-exact
passthrough + cassette v2 shape + key redaction; non-streaming JSON
single frame; two concurrent requests each forming its own
interaction; unknown route 404 with nothing written to disk.
