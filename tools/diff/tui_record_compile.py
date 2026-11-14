#!/usr/bin/env python3
"""Compile raw TUI session recordings into replayable channel-8
scenarios (diff-ext C2).

Input: `tui-diff-recording/1` raw files (suffix `.raw.json`, produced
by the env-gated oc-tui session recorder — `TUI_RECORD_DIR`, see
crates/oc-util/src/recorder.rs). Output: `tui-diff-scenario/1`
fixtures under `tools/diff/corpus/tui/recorded-local/` (git-ignored —
the PRIVACY iron law: recorded corpus NEVER enters a commit; reports
quote aggregate counts only).

Pipeline per recording:
  * endpoint census alignment (SCHEMA 端点普查): the recorded GET
    fetch map is kept verbatim (real bodies); any of the 21 census
    boot paths the session did not exercise is backfilled from the
    `home-empty` corpus fixture defaults (a replay must not stub_gap
    on boot endpoints the ref TUI probes but the recorded session
    never hit); `/api/event` is synthesized as the SSE stub with the
    recorded leading `server.connected` as `initial`.
  * event normalization: the recorded stream minus its leading
    `server.connected` becomes `events` (arrival order preserved),
    then `tools/diff/tui/w3_normalize.py`'s `normalize()` is applied
    verbatim (unique event ids, finish/completed backfill, ordinal
    correction, persisted-turn lifecycle pruning — same invariants,
    same code).
  * key passthrough: recorded `App::press` specs are mapped 1:1 onto
    the SCHEMA 键名表 vocabulary (`return`→`enter`, `escape`→`esc`,
    single chars→`raw:<c>`, named/ctrl/alt tokens unchanged).
  * redaction (default ON, `--no-redact` to disable — and idempotent:
    the recorder already redacts by default): message-content strings
    (keys text/title/summary/reasoning/output, arrays included) are
    replaced by shape-preserving placeholders — same length, same
    newlines/whitespace, same unicode letter/digit/other category per
    char — so both replay legs see identical shapes without the real
    words ever leaving the recording machine.

Usage:
  python3 tools/diff/tui_record_compile.py <rec.raw.json | dir>... \
      [-o tools/diff/corpus/tui/recorded-local] [--no-redact]
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent
sys.path.insert(0, str(REPO / "tools/diff/tui"))
import w3_normalize  # noqa: E402  (shared normalization, same invariants)

CORPUS = REPO / "tools/diff/corpus/tui"
DEFAULT_OUT = CORPUS / "recorded-local"

CENSUS_REF = CORPUS / "scenarios/home-empty.json"  # boot-stub defaults

# press-spec -> SCHEMA 键名表 token (everything else passes through)
KEY_ALIASES = {"return": "enter", "escape": "esc", " ": "space"}

CONTENT_KEYS = ("text", "title", "summary", "reasoning", "output")


def redact_char(ch: str) -> str:
    if ch.isspace():
        return ch
    if ch.isalpha():
        return "x" if ch.isascii() else "α"
    if ch.isdigit():
        return "9"
    return "-"


def redact_str(text: str) -> str:
    return "".join(redact_char(c) for c in text)


def redact(value):
    """Shape-preserving message-content redaction (idempotent)."""
    if isinstance(value, str):
        return value
    if isinstance(value, list):
        return [redact(v) for v in value]
    if isinstance(value, dict):
        return {k: (redact_content(v) if k in CONTENT_KEYS else redact(v))
                for k, v in value.items()}
    return value


def redact_content(value):
    if isinstance(value, str):
        return redact_str(value)
    if isinstance(value, list):
        return [redact_content(v) for v in value]
    if isinstance(value, dict):
        return {k: redact_content(v) for k, v in value.items()}
    return value


def key_token(spec: str) -> str:
    if spec in KEY_ALIASES:
        return KEY_ALIASES[spec]
    if len(spec) == 1:
        return f"raw:{spec}"
    return spec  # named / ctrl+<a-z> / alt+<dir>: SCHEMA vocabulary verbatim


def compile_recording(doc: dict, redact_on: bool) -> dict:
    if doc.get("schema") != "tui-diff-recording/1":
        raise SystemExit(f"bad recording schema: {doc.get('schema')!r}")
    events = list(doc.get("events") or [])
    # the leading server.connected is the SSE `initial`, not a replay event
    initial = {"id": "evt_connected", "type": "server.connected", "data": {}}
    if events and events[0].get("type") == "server.connected":
        initial = events.pop(0)

    boot = doc.get("boot") or {}
    sizes = doc.get("sizes") or []
    width = boot.get("width") or (sizes[0][0] if sizes else 80)
    height = boot.get("height") or (sizes[0][1] if sizes else 24)

    # fetch map: recorded GET endpoints verbatim; census backfill
    fetch: dict = {}
    for path, entry in (doc.get("fetch") or {}).items():
        if (entry or {}).get("method", "GET") != "GET":
            continue
        fetch[path] = {"status": entry.get("status", 200),
                       "body": entry.get("body")}
    if redact_on:
        fetch = redact(fetch)
    census = json.loads(CENSUS_REF.read_text())["fetch"]
    for path, entry in census.items():
        if path == "/api/event":
            continue
        fetch.setdefault(path, entry)
    # session-route backfill (SCHEMA Session-route convention): every
    # session id the recorded /api/session list names gets the route
    # endpoint set the ref boot probes (the rust census is known
    # asymmetric — an unstubbed ref probe is a stub_gap, never a diff).
    listed = ((fetch.get("/api/session") or {}).get("body") or {}).get("data") or []
    for row in listed:
        sid = row.get("id") if isinstance(row, dict) else None
        if not sid:
            continue
        base = f"/api/session/{sid}"
        fetch.setdefault(base, {"status": 200, "body": {"data": row}})
        for tail in ("message", "inbox", "permission", "form"):
            fetch.setdefault(f"{base}/{tail}", {"status": 200, "body": {"data": []}})
        fetch.setdefault(f"/api/experimental/session/{sid}/terminal",
                         {"status": 200, "body": {"data": []}})
    # /api/event: the SSE stub — body carries {"sse":true,"initial"}
    # (SCHEMA file-format field semantics).
    fetch["/api/event"] = {"status": 200,
                           "body": {"sse": True, "initial": initial}}

    scenario = {
        "schema": "tui-diff-scenario/1",
        "id": doc["id"],
        "width": width,
        "height": height,
        "fetch": fetch,
        "events": redact(events) if redact_on else events,
        "keys": [key_token(k) for k in doc.get("keys") or []],
        "settle_ms": 200,
        "recorded_sizes": sizes,  # superset field: the real size history
        "note": (f"compiled from real-session recording {doc['id']} "
                 f"(tools/diff/tui_record_compile.py; privacy: content "
                 "redacted, aggregates only)"),
    }
    # event normalization — w3_normalize's invariants verbatim (unique
    # ids, finish/completed backfill against the fetch wire, ordinal
    # correction, persisted-turn lifecycle pruning).
    w3_normalize.normalize(scenario)
    return scenario


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("inputs", nargs="+", help=".raw.json files or directories")
    ap.add_argument("-o", "--out", default=str(DEFAULT_OUT))
    ap.add_argument("--no-redact", action="store_true",
                    help="disable message-content redaction (local only)")
    args = ap.parse_args()
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)

    files: list[Path] = []
    for raw in args.inputs:
        path = Path(raw)
        if path.is_dir():
            files.extend(sorted(path.glob("*.raw.json")))
        else:
            files.append(path)
    if not files:
        raise SystemExit("no .raw.json recordings found")

    for path in files:
        doc = json.loads(path.read_text())
        scenario = compile_recording(doc, redact_on=not args.no_redact)
        dest = out / f"{scenario['id']}.json"
        dest.write_text(json.dumps(scenario, indent=2) + "\n")
        print(f"compiled {path.name} -> {dest} "
              f"(events={len(scenario['events'])} keys={len(scenario['keys'])} "
              f"redact={'off' if args.no_redact else 'on'})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
