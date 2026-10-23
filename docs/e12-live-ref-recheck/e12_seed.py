#!/usr/bin/env python3
"""Seed the E12 corpus sessions via the server's own HTTP API.

Reads session titles (exact bytes) from tools/parity/spec-canaries/
e12_search_corpus.json and POSTs /api/session with explicit ids:
parents get an explicit location; the child gets parentID only.
Prints each title's codepoint shape (NFC/NFD audit) and the server reply.
"""
import base64
import json
import sys
import time
import urllib.request

CORPUS = "/opt/opencode-rust/tools/parity/spec-canaries/e12_search_corpus.json"
DIRECTORY = "/tmp/opencode/e12ref-3500561-proj"


def shape(s: str) -> str:
    return ",".join(f"U+{ord(c):04X}" for c in s)


def main():
    base, user, password = sys.argv[1], sys.argv[2], sys.argv[3]
    directory = sys.argv[4] if len(sys.argv) > 4 else DIRECTORY
    with open(CORPUS) as fh:
        corpus = json.load(fh)
    token = base64.b64encode(f"{user}:{password}".encode()).decode()
    for s in corpus["sessions"]:
        body = {"id": s["id"], "title": s["title"]}
        if s.get("parent"):
            body["parentID"] = s["parent"]
        else:
            body["location"] = {"directory": directory}
        req = urllib.request.Request(
            base.rstrip("/") + "/api/session",
            data=json.dumps(body).encode("utf-8"),
            headers={"Content-Type": "application/json", "Authorization": f"Basic {token}"},
            method="POST",
        )
        try:
            with urllib.request.urlopen(req, timeout=10) as resp:
                out = json.loads(resp.read())
                info = out.get("data", {})
                print(
                    f"OK   {s['id']:12} title_cp=[{shape(s['title'])}] "
                    f"-> id={info.get('id')} parentID={info.get('parentID')}"
                )
        except Exception as exc:  # noqa: BLE001
            print(f"FAIL {s['id']:12} {exc}")
        time.sleep(0.15)  # distinct time_updated for stable desc ordering
    # verify listing sees them
    req = urllib.request.Request(
        base.rstrip("/") + "/api/session?limit=50&order=desc",
        headers={"Authorization": f"Basic {token}"},
    )
    with urllib.request.urlopen(req, timeout=10) as resp:
        out = json.loads(resp.read())
    rows = out.get("data", [])
    print(f"list: {len(rows)} rows: " + ", ".join(r["id"] for r in rows))


if __name__ == "__main__":
    main()
