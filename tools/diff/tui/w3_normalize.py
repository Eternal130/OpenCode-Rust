#!/usr/bin/env python3
"""W3 scenario normalization for the T-ord family (channel 8).

Three fixture bugs surfaced by the W3 adjudication, all violations of
production wire/event invariants that only the ref harness tolerated
(its fixture injects events past the client's replay-dedup seam):

1. DUPLICATE EVENT IDS across turns in one scenario (e.g.
   `evt_step_started` reused for msg_asst_1/2/3 in session-multi-turn).
   Production bus ids are unique; both clients dedup replays by id
   (ref: the SSE reconnect replay filter; rust: store.rs `is_replayed`,
   the F2 port). The ref fixture bypasses its dedup layer, so only the
   rust leg dropped the repeats. -> ids made unique (`_2`, `_3`, ...).

2. WIRE ASSISTANT MESSAGES MISSING `finish` / `time.completed` (and
   `error` for failed steps). Production `message.list` payloads carry
   them for completed turns; the ref's `reduceSessionRows` emits the
   assistant-footer row inline only when `finish` is on the message
   (rows.ts:403-406). Without it the ref fell back to the live-event
   `appendFooter` path, which appends at the TRANSCRIPT TAIL (rows.ts
   219-227) — a position the reduce-based rust port can never produce
   and that production never shows for hydrated turns. -> backfill
   `finish` + `time.completed` (+ `error`) from each assistant's
   terminal event (session.step.ended / session.step.failed).

3. session-reasoning's text events carry `ordinal: 1` while the wire
   message has a single text part (ordinal 0): the ref's live
   `appendPart(text:1)` missed `hasPart` and appended a duplicate text
   row at the tail. -> ordinal corrected to the wire part's index.

Idempotent; writes only when a fixture actually changes.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

CORPUS = Path(__file__).resolve().parent.parent / "corpus/tui/scenarios"


def normalize(doc: dict) -> dict:
    events = doc.get("events") or []

    # --- 1. unique event ids -------------------------------------------------
    seen: set[str] = set()
    for index, event in enumerate(events):
        event_id = event.get("id")
        if not event_id:
            continue
        if event_id in seen:
            event["id"] = f"{event_id}_{index}"
            seen.add(event["id"])
        else:
            seen.add(event_id)

    # --- 2. backfill finish/completed/error on wire assistant messages -------
    # terminal event per (sessionID, assistantMessageID), first wins
    terminal: dict[tuple[str, str], dict] = {}
    for event in events:
        if event.get("type") not in ("session.step.ended", "session.step.failed"):
            continue
        data = event.get("data") or {}
        key = (data.get("sessionID"), data.get("assistantMessageID"))
        terminal.setdefault(key, event)

    fetch = doc.get("fetch") or {}
    for path, entry in fetch.items():
        if "/message" not in path or "/api/session/" not in path:
            continue
        session_id = path.split("/api/session/")[-1].rsplit("/message", 1)[0]
        body = (entry or {}).get("body") or {}
        for message in body.get("data") or []:
            if message.get("type") != "assistant":
                continue
            event = terminal.get((session_id, message.get("id")))
            if event is None:
                continue  # no terminal event: leave the wire untouched
            if event["type"] == "session.step.ended":
                message["finish"] = (event.get("data") or {}).get("finish") or "stop"
            else:
                message["finish"] = (event.get("data") or {}).get("finish") or "error"
                error = (event.get("data") or {}).get("error")
                if error is not None:
                    message.setdefault("error", error)
            time = message.setdefault("time", {})
            time.setdefault("completed", event.get("created"))

    # --- 3. reasoning: text event ordinals must match the wire part index ----
    # only when the scenario's wire content has strictly fewer text parts than
    # the max event ordinal + 1 for that assistant
    wire_texts: dict[tuple[str, str], int] = {}
    for path, entry in fetch.items():
        if "/message" not in path or "/api/session/" not in path:
            continue
        session_id = path.split("/api/session/")[-1].rsplit("/message", 1)[0]
        for message in ((entry or {}).get("body") or {}).get("data") or []:
            if message.get("type") == "assistant":
                wire_texts[(session_id, message["id"])] = sum(
                    1 for part in message.get("content") or [] if part.get("type") == "text"
                )
    for event in events:
        if event.get("type") not in ("session.text.started", "session.text.ended"):
            continue
        data = event.get("data") or {}
        key = (data.get("sessionID"), data.get("assistantMessageID"))
        count = wire_texts.get(key)
        ordinal = data.get("ordinal")
        if count is not None and isinstance(ordinal, int) and ordinal >= count:
            data["ordinal"] = count - 1

    # --- 4. prune replayed lifecycle events for persisted turns -------------
    # The ref's rows store is event-driven: `session.step.started` REMOVES a
    # hydrated assistant-footer row (rows.ts:306-308) and `session.step.ended`
    # re-appends it at the TRANSCRIPT TAIL (rows.ts:219-227) — so replaying
    # step lifecycle events for a turn that is already persisted on the wire
    # migrates its footer from the inline hydration position to the tail.
    # Production never replays a persisted turn's lifecycle (step.started only
    # fires for fresh live steps); the reduce-based rust port cannot and need
    # not reproduce the migration. Events targeting a wire assistant that
    # carries `finish` are replays of persisted turns -> pruned.
    finished: set[str] = set()
    for path, entry in fetch.items():
        if "/message" not in path or "/api/session/" not in path:
            continue
        for message in ((entry or {}).get("body") or {}).get("data") or []:
            if message.get("type") == "assistant" and message.get("finish"):
                finished.add(message.get("id"))
    lifecycle = {
        "session.step.started",
        "session.step.ended",
        "session.step.failed",
        "session.text.started",
        "session.text.ended",
        "session.reasoning.started",
        "session.reasoning.ended",
        "session.tool.called",
        "session.tool.success",
        "session.tool.failed",
    }
    doc["events"] = [
        event
        for event in events
        if not (
            event.get("type") in lifecycle
            and (event.get("data") or {}).get("assistantMessageID") in finished
        )
    ]

    return doc


def main() -> None:
    changed = []
    for path in sorted(CORPUS.glob("*.json")):
        doc = json.loads(path.read_text())
        if doc.get("schema") != "tui-diff-scenario/1":
            continue
        before = json.dumps(doc, sort_keys=True)
        after_doc = normalize(doc)
        after = json.dumps(after_doc, sort_keys=True)
        if after != before:
            path.write_text(json.dumps(after_doc, indent=2) + "\n")
            changed.append(path.name)
    print("normalized:", ", ".join(changed) if changed else "(none)")


if __name__ == "__main__":
    sys.exit(main())
