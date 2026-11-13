#!/usr/bin/env python3
"""Mechanically mine ref-test fixtures into the six differential corpora.

Pure-addition: writes only `mined-*` files (plus corpus/http/mined-http.json
consumed by tools/diff/http_full.py's embedded-corpus loader). Existing
corpus files, drivers, and orchestrators are never modified. Re-running
produces byte-identical output: every channel first deletes its own
`mined-*` files, then regenerates deterministically from the ref checkout.

Sources (read-only, /opt/opencode-ref default):
  ch1 config   : packages/core/test/config/config.test.ts
                 (JSON.stringify / writeFile string-literal args, regex +
                 balanced-paren extraction, evaluated with bun), plus
                 hand-transcribed decode inputs from
                 packages/schema/test/config.test.ts and one doc from
                 packages/server/test/config.test.ts
  ch2 ACP      : packages/cli/test/acp/translate.test.ts — input event wire
                 objects of every `events:` row (helpers mirrored from
                 test/acp/wire-fixture.ts; expected outputs are NOT mined,
                 the differential compares both sides on its own).
                 `messages:` (replay) rows are outside this channel's
                 corpus shape ({name,ctx,events,abandon}) — declared
                 unminable here.
  ch3 sse      : three supplementary event sequences serialized from the
                 ACP rows (tool lifecycle / reasoning-ordinal interleave /
                 compaction stream)
  ch4 schema   : packages/core/test/plugin/tool-input-repair.test.ts
                 (raw schema, pre-repair input) pairs — raw-input verdicts
                 remain meaningful for the validator differential
  ch5 compaction: packages/ai/test/provider/explicit-compaction.test.ts
                 inline response.compaction documents
  ch6 http     : packages/server/test/*.test.ts request shapes ->
                 corpus/http/mined-http.json (error paths + POST bodies)

Usage: python3 tools/diff/mine_ref_fixtures.py [--ref /opt/opencode-ref]
"""
from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent
DIFF = REPO / "tools/diff"
CORPUS = DIFF / "corpus"

TOKENS1 = {"input": 1, "output": 1, "reasoning": 0, "cache": {"read": 0, "write": 0}}


def write_files(out_dir: Path, files: dict, pattern: str) -> int:
    out_dir.mkdir(parents=True, exist_ok=True)
    for old in out_dir.glob(pattern):
        old.unlink()
    for name, text in sorted(files.items()):
        (out_dir / name).write_text(text)
    return len(files)


def slug(text: str, limit: int = 48) -> str:
    s = re.sub(r"[^a-z0-9]+", "-", text.lower()).strip("-")
    return s[:limit].strip("-")


# ---------------------------------------------------------------- ch1 config

def extract_balanced(text: str, start: int, open_ch: str, close_ch: str) -> str:
    # `start` sits just AFTER the opening delimiter: depth begins at 1.
    depth, i, in_str, quote, esc = 1, start, False, "", False
    while i < len(text):
        c = text[i]
        if in_str:
            if esc:
                esc = False
            elif c == "\\":
                esc = True
            elif c == quote:
                in_str = False
        else:
            if c in "\"'`":
                in_str, quote = True, c
            elif c == open_ch:
                depth += 1
            elif c == close_ch:
                depth -= 1
                if depth == 0:
                    return text[start:i]
        i += 1
    return text[start:i]


def scan_string(text: str, quote: str) -> str:
    """Return the quoted literal starting at text[0] (quote char included)."""
    i, esc = 1, False
    while i < len(text):
        c = text[i]
        if esc:
            esc = False
        elif c == "\\":
            esc = True
        elif c == quote:
            return text[:i + 1]
        i += 1
    return text


def eval_literals(args: list) -> list:
    """Evaluate JS object-literal snippets with bun; None on failure."""
    if not args:
        return []
    payload = json.dumps(args)
    script = (
        "const args = JSON.parse(require('fs').readFileSync(0,'utf8'));"
        "const out = args.map((a) => {"
        "  try { const v = eval('(' + a + ')');"
        "    return JSON.stringify(v) === undefined ? null : JSON.parse(JSON.stringify(v));"
        "  } catch { return null; }"
        "});"
        "console.log(JSON.stringify(out));"
    )
    res = subprocess.run(["bun", "-e", script], input=payload,
                         capture_output=True, text=True)
    if res.returncode != 0:
        return [None] * len(args)
    return json.loads(res.stdout)


def mine_config(ref: Path) -> int:
    src = (ref / "packages/core/test/config/config.test.ts").read_text()
    snippets: list[tuple[str, str]] = []  # (code, kind)

    for m in re.finditer(r"JSON\.stringify\(", src):
        arg = extract_balanced(src, m.end(), "(", ")")
        snippets.append((arg, "js"))

    # writeFile direct string-literal / comment-bearing template docs
    for m in re.finditer(r"writeFile\(\s*[^,]+,\s*", src):
        rest = src[m.end():].lstrip()
        if rest.startswith("`"):
            arg = scan_string(rest, "`")
            inner = arg[1:-1]
            if "${" in inner:
                continue  # interpolated with variables — skip
            snippets.append(("JSON.stringify(`" + inner + "`)", "js"))
        elif rest[:1] in "\"'":
            arg = scan_string(rest, rest[0])
            snippets.append(("JSON.stringify(" + arg + ")", "js"))

    values = eval_literals([code for code, _ in snippets])
    seen = set()
    # existing corpus contents (parse-tolerant: raw text fallback)
    existing = set()
    cfg_dir = CORPUS / "config"
    for f in cfg_dir.glob("*[0-9]*.jso*"):
        if f.name.startswith("mined-"):
            continue  # our own previous output is not "existing" corpus
        text = f.read_text()
        try:
            existing.add(json.dumps(json.loads(text), sort_keys=True))
        except Exception:
            existing.add(text.strip())
    for v in values:
        if v is None:
            continue
        if isinstance(v, (dict, list)):
            seen.add(json.dumps(v, sort_keys=True))
        else:
            seen.add(json.dumps(v))
    candidates: list = []
    for v in values:
        if v is None:
            continue
        key = json.dumps(v, sort_keys=True) if isinstance(v, (dict, list)) else json.dumps(v)
        if key in existing:
            continue
        if not isinstance(v, dict) or not v:
            continue  # config docs are non-empty objects (null/"" already in corpus)
        candidates.append(v)
        existing.add(key)

    # hand-transcribed decode inputs — packages/schema/test/config.test.ts
    # (lines 13-53) and packages/server/test/config.test.ts (lines 20-31)
    transcribed = [
        # schema config.test.ts:14-20
        {"worktree": {"directory": "../worktrees"}},
        {"worktree": {}},
        {"worktree": {"directory": " "}},
        {"worktree": {"directory": False}},
        # schema config.test.ts:22-27
        {"providers": {"console-anthropic": {"canonical": "anthropic"}}},
        {"providers": {"custom": {"canonical": 1}}},
        # schema config.test.ts:30-45
        {"providers": {"custom": {"settings": {
            "timeout": 900000, "headerTimeout": 600000, "chunkTimeout": False}}}},
        {"providers": {"custom": {"settings": {
            "headerTimeout": False, "chunkTimeout": 600000}}}},
        # schema config.test.ts:47-53
        {"websearch": False},
        {"websearch": {"provider": "exa"}},
        {"websearch": {"provider": "random"}},
        # schema config.test.ts:60-66 (permissions doc)
        {"permissions": [
            {"action": "shell", "resource": "*", "effect": "ask"},
            {"action": "shell", "resource": "git status", "effect": "allow"},
        ]},
        # server config.test.ts:23-29 (permissions + mcp doc)
        {"permissions": [
            {"action": "shell", "resource": "*", "effect": "ask"},
            {"action": "shell", "resource": "git status", "effect": "allow"},
        ], "mcp": {"servers": {"docs": {"type": "remote", "url": "https://example.com/mcp"}}}},
    ]
    for v in transcribed:
        key = json.dumps(v, sort_keys=True)
        if key not in existing:
            candidates.append(v)
            existing.add(key)

    files = {}
    for i, v in enumerate(sorted(candidates, key=lambda x: json.dumps(x, sort_keys=True)), 1):
        keys = "+".join(sorted(v.keys())) if isinstance(v, dict) else "doc"
        name = f"mined-config-{i:03d}-{slug(keys)}.json"
        files[name] = json.dumps(v, ensure_ascii=False, indent=1) + "\n"
    # jsonc fixture with comments — server config.test.ts:71-77
    # (write_files already wiped prior mined-* output; always regenerate)
    jsonc = (
        "{\n"
        "  // keep this comment\n"
        '  "model": "provider/model",\n'
        '  "shell": "bash"\n'
        "}\n"
    )
    files[f"mined-config-{len(files) + 1:03d}-comments-model-shell.jsonc"] = jsonc
    return write_files(cfg_dir, files, "mined-*")


# ------------------------------------------------------------------- ch2 ACP
# helpers mirrored 1:1 from ref packages/cli/test/acp/wire-fixture.ts

ROOT = "ses_root"


def ev(type_, data, eid):
    return {"id": eid, "type": type_, "data": data}


def delivered(s, i, eid):
    return ev("session.inbox.delivered", {"sessionID": s, "inboxID": i}, eid)


def succeeded(s, eid):
    return ev("session.execution.succeeded", {"sessionID": s}, eid)


def failed(s, error, eid):
    return ev("session.execution.failed", {"sessionID": s, "error": error}, eid)


def text_delta(s, m, d, o=0, eid=None):
    return ev("session.text.delta", {"sessionID": s, "assistantMessageID": m, "ordinal": o, "delta": d}, eid)


def reasoning_delta(s, m, d, o=0, eid=None):
    return ev("session.reasoning.delta", {"sessionID": s, "assistantMessageID": m, "ordinal": o, "delta": d}, eid)


def step_ended(s, m, finish="stop", tokens=None, eid=None):
    return ev("session.step.ended", {
        "sessionID": s, "assistantMessageID": m, "finish": finish,
        "cost": 0, "tokens": tokens or dict(TOKENS1),
    }, eid)


def child_created(s, parent, title, eid):
    return ev("session.created", {
        "sessionID": s, "slug": s, "projectID": "project",
        "location": {"directory": "/workspace"}, "parentID": parent,
        "title": title, "version": "test",
    }, eid)


def tool_started(s, cid, name, eid):
    return ev("session.tool.input.started",
              {"sessionID": s, "assistantMessageID": "msg_tools", "id": cid, "name": name}, eid)


def tool_called(s, cid, inp, eid):
    return ev("session.tool.called",
              {"sessionID": s, "assistantMessageID": "msg_tools", "id": cid,
               "input": inp, "executed": False}, eid)


def tool_progress(s, cid, metadata, eid):
    return ev("session.tool.progress",
              {"sessionID": s, "assistantMessageID": "msg_tools", "id": cid,
               "metadata": metadata}, eid)


def tool_succeeded(s, cid, metadata, text, eid):
    return ev("session.tool.success", {
        "sessionID": s, "assistantMessageID": "msg_tools", "id": cid,
        "metadata": metadata, "content": [{"type": "text", "text": text}],
        "executed": True,
    }, eid)


def tool_failed(s, cid, error=None, metadata=None, content=None, eid=None):
    data = {"sessionID": s, "assistantMessageID": "msg_tools", "id": cid, "executed": True}
    if error is not None:
        data["error"] = error
    if metadata is not None:
        data["metadata"] = metadata
    if content is not None:
        data["content"] = content
    return ev("session.tool.failed", data, eid)


def compaction_started(s=ROOT, eid=None):
    return ev("session.compaction.started",
              {"sessionID": s, "reason": "manual", "recent": "", "inputID": "msg_compact"}, eid)


def compaction_delta(text, s=ROOT, eid=None):
    return ev("session.compaction.delta", {"sessionID": s, "text": text}, eid)


def compaction_ended(text, s=ROOT, eid=None):
    return ev("session.compaction.ended",
              {"sessionID": s, "reason": "manual", "text": text, "recent": ""}, eid)


def compaction_failed(error, reason="manual", s=ROOT, eid=None):
    return ev("session.compaction.failed",
              {"sessionID": s, "reason": reason, "error": error}, eid)


PROVIDER_ERROR = {"type": "provider.error", "message": "summary request failed"}


def usage(i, o, r=0, read=0, write=0):
    return {"input": i, "output": o, "reasoning": r, "cache": {"read": read, "write": write}}


CTX = {"sessionID": ROOT, "cwd": "/workspace",
       "start": {"type": "input", "id": "msg_input"},
       "childUpdates": False, "compaction": False}

# rows transcribed from packages/cli/test/acp/translate.test.ts:78-596 —
# input wire events only; provenance = row name in that file.
ACQ_ROWS = [
    ("ignores until own input delivered", {},
     lambda e: [text_delta(ROOT, "msg_early", "early", 0, e()),
                delivered(ROOT, "msg_other_input", e()),
                succeeded(ROOT, e())], False),
    ("ignores other sessions events", {},
     lambda e: [delivered(ROOT, "msg_input", e()),
                delivered("ses_other", "msg_input", e()),
                text_delta("ses_other", "msg_other", "other", 0, e()),
                text_delta(ROOT, "msg_ok", "accepted", 0, e())], False),
    ("text reasoning one message id per ordinal", {},
     lambda e: [delivered(ROOT, "msg_input", e()),
                reasoning_delta(ROOT, "msg_a", "think", 0, e()),
                text_delta(ROOT, "msg_a", "answer", 1, e()),
                reasoning_delta(ROOT, "msg_a", "more", 1, e())], False),
    ("normal turn end turn", {},
     lambda e: [delivered(ROOT, "msg_input", e()),
                step_ended(ROOT, "msg_1", eid=e()), succeeded(ROOT, e())], False),
    ("length limited step max tokens", {},
     lambda e: [delivered(ROOT, "msg_input", e()),
                step_ended(ROOT, "msg_1", "length", eid=e()), succeeded(ROOT, e())], False),
    ("content filtered step refusal", {},
     lambda e: [delivered(ROOT, "msg_input", e()),
                step_ended(ROOT, "msg_1", "content-filter", eid=e()), succeeded(ROOT, e())], False),
    ("content filter failure refusal no error", {},
     lambda e: [delivered(ROOT, "msg_input", e()),
                failed(ROOT, {"type": "provider.content-filter", "message": "blocked"}, e())], False),
    ("server side interruption cancelled", {},
     lambda e: [delivered(ROOT, "msg_input", e()),
                ev("session.execution.interrupted", {"sessionID": ROOT, "reason": "shutdown"}, e())], False),
    ("aborted execution cancelled no error", {},
     lambda e: [delivered(ROOT, "msg_input", e()),
                failed(ROOT, {"type": "aborted", "message": "interrupted"}, e())], False),
    ("execution auth failure auth required", {},
     lambda e: [delivered(ROOT, "msg_input", e()),
                failed(ROOT, {"type": "provider.auth", "message": "missing key"}, e())], False),
    ("step auth failure auth required", {},
     lambda e: [delivered(ROOT, "msg_input", e()),
                ev("session.step.failed", {"sessionID": ROOT, "assistantMessageID": "msg_1",
                                           "error": {"type": "provider.auth", "message": "expired"}}, e()),
                succeeded(ROOT, e())], False),
    ("other execution failures service failures", {},
     lambda e: [delivered(ROOT, "msg_input", e()),
                failed(ROOT, {"type": "provider.rate-limit", "message": "slow down"}, e())], False),
    ("sums usage across steps thought cache tokens", {},
     lambda e: [delivered(ROOT, "msg_input", e()),
                step_ended(ROOT, "msg_1", "tool-calls", usage(100, 10, 0, 0, 50), e()),
                step_ended(ROOT, "msg_2", "stop", usage(20, 5, 3, 150, 0), e()),
                succeeded(ROOT, e())], False),
    ("failed step tokens cleared on next start", {},
     lambda e: [delivered(ROOT, "msg_input", e()),
                ev("session.step.failed", {"sessionID": ROOT, "assistantMessageID": "msg_1",
                                           "error": {"type": "provider.stream", "message": "stream interrupted"},
                                           "cost": 0, "tokens": usage(40, 4)}, e()),
                ev("session.step.started", {"sessionID": ROOT, "assistantMessageID": "msg_2",
                                            "agent": "build",
                                            "model": {"providerID": "test", "id": "test-model"},
                                            "started": 0}, e()),
                step_ended(ROOT, "msg_2", "stop", usage(20, 7), e()),
                succeeded(ROOT, e())], False),
    ("excludes child steps from turn usage", {},
     lambda e: [delivered(ROOT, "msg_input", e()),
                child_created("ses_child", ROOT, "Explore", e()),
                step_ended("ses_child", "msg_child", "stop", usage(500, 50), e()),
                succeeded("ses_child", e()),
                step_ended(ROOT, "msg_root", "stop", usage(20, 7), e()),
                succeeded(ROOT, e())], False),
    ("tool call pending progress completion", {},
     lambda e: [delivered(ROOT, "msg_input", e()),
                tool_started(ROOT, "call_ok", "shell", e()),
                tool_called(ROOT, "call_ok", {"command": "printf done", "workdir": "sub"}, e()),
                tool_progress(ROOT, "call_ok", {"phase": 1}, e()),
                tool_succeeded(ROOT, "call_ok", {"exit": 0}, "done", e())], False),
    ("tool call failed partial content error", {},
     lambda e: [delivered(ROOT, "msg_input", e()),
                tool_started(ROOT, "call_fail", "read", e()),
                tool_called(ROOT, "call_fail", {"path": "/workspace/missing.ts"}, e()),
                tool_failed(ROOT, "call_fail", error={"type": "tool.error", "message": "not found"},
                            metadata={"bytes": 0}, content=[{"type": "text", "text": "opening"}], eid=e())], False),
    ("edit tool file location", {},
     lambda e: [delivered(ROOT, "msg_input", e()),
                tool_started(ROOT, "call_edit", "edit", e()),
                tool_called(ROOT, "call_edit", {"path": "/workspace/a.ts", "oldString": "a", "newString": "b"}, e()),
                tool_succeeded(ROOT, "call_edit", {}, "edited", e())], False),
    ("write tool file location", {},
     lambda e: [delivered(ROOT, "msg_input", e()),
                tool_started(ROOT, "call_write", "write", e()),
                tool_called(ROOT, "call_write", {"path": "/workspace/b.ts", "content": "b"}, e()),
                tool_succeeded(ROOT, "call_write", {}, "written", e())], False),
    ("patch tool file location", {},
     lambda e: [delivered(ROOT, "msg_input", e()),
                tool_started(ROOT, "call_patch", "patch", e()),
                tool_called(ROOT, "call_patch", {
                    "patchText": "*** Begin Patch\n*** Update File: /workspace/c.ts\n@@\n-one\n+two\n*** End Patch",
                }, e()),
                tool_succeeded(ROOT, "call_patch", {}, "patched", e())], False),
    ("child tool call projected with prefix", {"childUpdates": False},
     lambda e: [delivered(ROOT, "msg_input", e()),
                child_created("ses_child", ROOT, "Explore", e()),
                tool_started("ses_child", "call_1", "read", e())], False),
    ("nested child depth own parent", {},
     lambda e: [delivered(ROOT, "msg_input", e()),
                child_created("ses_child", ROOT, "Explore", e()),
                child_created("ses_grandchild", "ses_child", "Deeper", e()),
                child_created("ses_stranger", "ses_unknown", "Unrelated", e()),
                tool_started("ses_grandchild", "call_1", "read", e()),
                text_delta("ses_stranger", "msg_stranger", "ignored", 0, e())], False),
    ("child work routed to extension", {"childUpdates": True},
     lambda e: [delivered(ROOT, "msg_input", e()),
                child_created("ses_child", ROOT, "Explore", e()),
                ev("session.execution.started", {"sessionID": "ses_child"}, e()),
                text_delta("ses_child", "msg_child", "nested", 0, e()),
                failed("ses_child", {"type": "tool.error", "message": "boom"}, e())], False),
    ("cancelled turn fails open tools incl child", {},
     lambda e: [delivered(ROOT, "msg_input", e()),
                child_created("ses_child", ROOT, "Explore", e()),
                tool_started(ROOT, "call_root", "shell", e()),
                tool_called(ROOT, "call_root", {"command": "sleep 60"}, e()),
                tool_progress(ROOT, "call_root", {"pid": 1}, e()),
                tool_started("ses_child", "call_child", "read", e())], True),
    ("cancelled turn cancels open compaction", {"compaction": True},
     lambda e: [delivered(ROOT, "msg_input", e()), compaction_started(eid=e())], True),
    ("compaction stream summary completes", {"compaction": True},
     lambda e: [delivered(ROOT, "msg_input", e()),
                compaction_started(eid=e()), compaction_delta("Sum", eid=e()),
                compaction_delta("mary", eid=e()), compaction_ended("Summary", eid=e())], False),
    ("compaction completes without summary", {"compaction": True},
     lambda e: [delivered(ROOT, "msg_input", e()),
                compaction_started(eid=e()), compaction_ended("", eid=e())], False),
    ("compaction failed clears partial summary", {"compaction": True},
     lambda e: [delivered(ROOT, "msg_input", e()),
                compaction_started(eid=e()), compaction_delta("partial", eid=e()),
                compaction_failed(PROVIDER_ERROR, eid=e())], False),
    ("compaction core failure message kept", {"compaction": True},
     lambda e: [delivered(ROOT, "msg_input", e()),
                compaction_started(eid=e()),
                compaction_failed({"type": "compaction.failed", "message": "Output limit"}, eid=e())], False),
    ("compaction defect generic failure", {"compaction": True},
     lambda e: [delivered(ROOT, "msg_input", e()),
                compaction_started(eid=e()),
                compaction_failed({"type": "compaction.failed",
                                   "message": "Error: locked\n    at run (compaction.ts:1:1)"}, eid=e())], False),
    ("interrupted compaction cancelled", {"compaction": True},
     lambda e: [delivered(ROOT, "msg_input", e()),
                compaction_started(eid=e()),
                compaction_failed({"type": "compaction.interrupted", "message": "interrupted"}, eid=e())], False),
    ("aborted compaction cancelled", {"compaction": True},
     lambda e: [delivered(ROOT, "msg_input", e()),
                compaction_started(eid=e()),
                compaction_failed({"type": "aborted", "message": "Compaction cancelled"}, eid=e())], False),
    ("compaction fails before starts settles", {"compaction": True},
     lambda e: [delivered(ROOT, "msg_input", e()),
                compaction_failed({"type": "compaction.unavailable",
                                   "message": "Nothing to compact yet"}, "auto", e())], False),
    ("summary chunks outside open compaction dropped", {"compaction": True},
     lambda e: [delivered(ROOT, "msg_input", e()),
                compaction_delta("early", eid=e()), compaction_started(eid=e()),
                compaction_ended("Summary", eid=e()), compaction_delta("late", eid=e())], False),
    ("compaction end without start dropped", {"compaction": True},
     lambda e: [delivered(ROOT, "msg_input", e()), compaction_ended("Summary", eid=e())], False),
    ("compaction marker without capability", {},
     lambda e: [delivered(ROOT, "msg_input", e()),
                compaction_started(eid=e()), compaction_delta("Summary", eid=e()),
                compaction_ended("Summary", eid=e())], False),
    ("failed compaction marker full error", {},
     lambda e: [delivered(ROOT, "msg_input", e()),
                compaction_started(eid=e()), compaction_failed(PROVIDER_ERROR, eid=e())], False),
    ("pre-start compaction failure marker", {},
     lambda e: [delivered(ROOT, "msg_input", e()),
                compaction_failed(PROVIDER_ERROR, "auto", e())], False),
    ("child compaction projected as marker", {"compaction": True},
     lambda e: [delivered(ROOT, "msg_input", e()),
                child_created("ses_child", ROOT, "Explore", e()),
                compaction_started("ses_child", e()), compaction_ended("Summary", "ses_child", e())], False),
    ("child compaction via extension", {"compaction": True, "childUpdates": True},
     lambda e: [delivered(ROOT, "msg_input", e()),
                child_created("ses_child", ROOT, "Explore", e()),
                compaction_started("ses_child", e()), compaction_ended("Summary", "ses_child", e())], False),
]


def mine_acp() -> int:
    files = {}
    for i, (name, ctx_over, build, abandon) in enumerate(ACQ_ROWS, 1):
        ctx = dict(CTX)
        ctx.update(ctx_over)
        counter = iter(range(1, 200))
        events = build(lambda: f"evt_{next(counter)}")
        rec = {"name": f"mined-{name}", "ctx": ctx, "events": events}
        if abandon:
            rec["abandon"] = True
        fname = f"mined-{i:03d}-{slug(name)}.json"
        files[fname] = json.dumps(rec, ensure_ascii=False, indent=1) + "\n"
    return write_files(CORPUS / "acp", files, "mined-*")


# --------------------------------------------------------------- ch3 sse proj

def sse_doc(name: str, description: str, events: list) -> tuple:
    stamped = []
    for n, e in enumerate(events, 1):
        stamped.append({"id": f"evt_m{n:03d}", "created": 2000 + n,
                        "type": e["type"], "data": e["data"]})
    doc = {"name": name, "description": description, "events": stamped}
    return (f"{name}.json", json.dumps(doc, ensure_ascii=False, indent=1) + "\n")


def mine_sse() -> int:
    files = {}
    counter = iter(range(1, 200))
    e = lambda: f"evt_{next(counter)}"  # noqa: E731
    step_started = ev("session.step.started",
                      {"sessionID": ROOT, "assistantMessageID": "msg_asst_1", "agent": "build",
                       "model": {"providerID": "test", "id": "test-model"}, "started": 1000}, e())
    seqs = [
        sse_doc("mined-40-tool-lifecycle",
                "ref translate.test.ts 'streams a tool call...'（补 step.started 锚）: pending → called → progress → success",
                [step_started,
                 tool_started(ROOT, "call_ok", "shell", e()),
                 tool_called(ROOT, "call_ok", {"command": "printf done", "workdir": "sub"}, e()),
                 tool_progress(ROOT, "call_ok", {"phase": 1}, e()),
                 tool_succeeded(ROOT, "call_ok", {"exit": 0}, "done", e())]),
        sse_doc("mined-41-reasoning-text-ordinals",
                "ref translate.test.ts 'streams text and reasoning...'（补锚）: 同一消息 reasoning/text 交错 ordinal",
                [step_started,
                 reasoning_delta(ROOT, "msg_asst_1", "think", 0, e()),
                 text_delta(ROOT, "msg_asst_1", "answer", 1, e()),
                 reasoning_delta(ROOT, "msg_asst_1", "more", 1, e())]),
        sse_doc("mined-42-compaction-stream",
                "ref translate.test.ts 'streams a compaction summary...'（补锚）: started → delta×2 → ended",
                [step_started,
                 compaction_started(eid=e()), compaction_delta("Sum", eid=e()),
                 compaction_delta("mary", eid=e()), compaction_ended("Summary", eid=e())]),
    ]
    files.update(dict(seqs))
    return write_files(CORPUS / "sse_projection", files, "mined-*")


# ------------------------------------------------------------- ch4 json-schema

def obj(properties, required=None):
    s = {"type": "object", "properties": properties}
    if required:
        s["required"] = required
    return s


def schema_case(name: str, schema: dict, instances: list, keywords: list) -> tuple:
    doc = {"name": name, "keywords": keywords, "schema": schema,
           "instances": [{"label": f"in{i}", "value": v} for i, v in enumerate(instances, 1)]}
    return (f"{name}.json", json.dumps(doc, ensure_ascii=False, indent=1) + "\n")


def mine_json_schema() -> int:
    files = {}
    cases = [
        # tool-input-repair.test.ts:51-68
        schema_case("mined-repair-valid-identity",
                    obj({"count": {"type": "integer"},
                         "nested": obj({"enabled": {"type": "boolean"}}),
                         "items": {"type": "array", "items": {"type": "number"}}}),
                    [{"count": 2, "nested": {"enabled": True}, "items": [2, 3], "extra": "keep"}],
                    ["type", "properties", "items"]),
        # :71-89
        schema_case("mined-repair-stringified-root",
                    obj({"count": {"type": "integer"},
                         "item": obj({"enabled": {"type": "boolean"}}),
                         "list": {"type": "array", "items": {"type": "integer"}}}),
                    ['{"count":"2","item":"{\\"enabled\\":\\"false\\"}","list":"[\\"3\\"]"}',
                     "{broken", "[]", None],
                    ["type", "properties", "items"]),
        # :92-124
        schema_case("mined-repair-extras-closed-objects",
                    {**obj({"known": {"type": "integer"},
                            "closed": {**obj({"keep": {"type": "integer"}}), "additionalProperties": False},
                            "open": obj({"keep": {"type": "integer"}}),
                            "items": {"type": "array",
                                      "items": {**obj({"keep": {"type": "integer"}}),
                                                "additionalProperties": False}}}),
                     "additionalProperties": False},
                    [{"known": "2", "extra": True,
                      "closed": {"keep": "3", "extra": True},
                      "open": {"keep": "4", "extra": True},
                      "items": [{"keep": "5", "extra": True}]},
                     {"extra": True}],
                    ["type", "properties", "additionalProperties", "items"]),
        # :127-139
        schema_case("mined-repair-pattern-ownership",
                    {**obj({"known": {"type": "integer"}}),
                     "additionalProperties": False,
                     "patternProperties": {"^match$": {"type": "integer"}}},
                    [{"known": 1, "match": "2", "extra": True}],
                    ["type", "properties", "additionalProperties", "patternProperties"]),
        # :142-155 (three composition keywords, same instance)
        schema_case("mined-repair-composed-ownership-allof",
                    {"type": "object", "allOf": [obj({"name": {"type": "string"}})],
                     "additionalProperties": False},
                    [{"name": "example", "extra": True}], ["type", "allOf", "additionalProperties"]),
        schema_case("mined-repair-composed-ownership-anyof",
                    {"type": "object", "anyOf": [obj({"name": {"type": "string"}})],
                     "additionalProperties": False},
                    [{"name": "example", "extra": True}], ["type", "anyOf", "additionalProperties"]),
        schema_case("mined-repair-composed-ownership-oneof",
                    {"type": "object", "oneOf": [obj({"name": {"type": "string"}})],
                     "additionalProperties": False},
                    [{"name": "example", "extra": True}], ["type", "oneOf", "additionalProperties"]),
        # :158-209
        schema_case("mined-repair-optional-nulls-placeholders",
                    obj({"optional": {"type": "string"},
                         "required": {"type": "string"},
                         "nullable": {"type": "string", "nullable": True},
                         "union": {"anyOf": [{"type": "integer"}, {"type": "null"}]},
                         "constant": {"anyOf": [{"type": "integer"}, {"const": None}]},
                         "permissive": {"anyOf": [{"type": "integer"}, True]},
                         "referenced": {"anyOf": [{"type": "integer"}, {"$ref": "#/$defs/nullable"}]},
                         "placeholder": {"type": "integer"},
                         "array": {"type": "array", "items": {"type": "string"}},
                         "requiredPlaceholder": {"type": "boolean"},
                         "object": {"type": "object"},
                         "unknown": {}},
                        ["required", "requiredPlaceholder"]),
                    [{"optional": None, "required": None, "nullable": None, "union": None,
                      "constant": None, "permissive": None, "referenced": None,
                      "placeholder": {}, "array": {}, "requiredPlaceholder": {},
                      "object": {}, "unknown": None}],
                    ["type", "required", "properties", "anyOf", "const", "$ref", "items"]),
        # :211-225
        schema_case("mined-repair-composition-hidden-nulls",
                    obj({"wrapped": {"anyOf": [{"type": ["string", "null"]}]},
                         "enumerated": {"anyOf": [{"enum": [None, "keep"]}]},
                         "permissive": {"anyOf": [{}]},
                         "constant": {"type": "string", "const": "keep"}}),
                    [{"wrapped": None, "enumerated": None, "permissive": None, "constant": None}],
                    ["type", "anyOf", "enum", "const"]),
        # :227-236
        schema_case("mined-repair-allof-required-parent",
                    {**obj({"value": {"type": "string"}}), "allOf": [{"required": ["value"]}]},
                    [{"value": None}], ["type", "properties", "allOf", "required"]),
        # :238-269
        schema_case("mined-repair-numeric-boolean-coercion",
                    obj({"number": {"type": "number"}, "integer": {"type": "integer"},
                         "enabled": {"type": "boolean"}, "disabled": {"type": "boolean"},
                         "valid": {"type": "integer"}, "empty": {"type": "number"},
                         "infinite": {"type": "number"}, "fractional": {"type": "integer"},
                         "unsafe": {"type": "integer"}, "uppercase": {"type": "boolean"}}),
                    [{"number": "1.5", "integer": "42", "enabled": "true", "disabled": "false",
                      "valid": 3, "empty": " ", "infinite": "Infinity", "fractional": "1.5",
                      "unsafe": "9007199254740992", "uppercase": "TRUE"}],
                    ["type"]),
        # :272-304
        schema_case("mined-repair-scalar-wrapping",
                    obj({"text": {"type": "array", "items": {"type": "string"}},
                         "integer": {"type": "array", "items": {"type": "integer"}},
                         "boolean": {"type": "array", "items": {"type": "boolean"}},
                         "item": {"type": "array", "items": obj({"count": {"type": "integer"}})},
                         "incompatible": {"type": "array", "items": {"type": "string"}},
                         "fractional": {"type": "array", "items": {"type": "integer"}},
                         "unconstrained": {"type": "array"}}),
                    [{"text": "one", "integer": "42", "boolean": "false",
                      "item": '{"count":"2"}', "incompatible": 2, "fractional": 1.5,
                      "unconstrained": "4"}],
                    ["type", "items"]),
        # :307-333
        schema_case("mined-repair-question-like-nested",
                    obj({"questions": {"type": "array",
                                       "items": obj({"question": {"type": "string"},
                                                     "multiple": {"type": "boolean"},
                                                     "options": {"type": "array",
                                                                 "items": obj({"label": {"type": "string"},
                                                                               "description": {"type": "string"}},
                                                                              ["label"])}})}}),
                    [{"questions": [{"question": "Pick one", "multiple": "false",
                                     "options": {"label": "First", "description": None}}]}],
                    ["type", "properties", "items", "required"]),
        # :336-361
        schema_case("mined-repair-nullable-unions",
                    obj({"number": {"anyOf": [{"type": "number"}, {"type": "null"}]},
                         "boolean": {"oneOf": [{"type": "boolean"}, {"type": "null"}]},
                         "nullable": {"anyOf": [{"type": "number"}, {"type": "null"}]},
                         "typed": {"type": ["integer", "null"]},
                         "typedBoolean": {"type": ["boolean", "null"]},
                         "accepted": {"anyOf": [{"type": "string"}, {"type": "number"}]},
                         "valid": {"type": ["number", "null"]}}),
                    [{"number": "2", "boolean": "false", "nullable": None, "typed": "3",
                      "typedBoolean": "true", "accepted": "4", "valid": 5}],
                    ["type", "anyOf", "oneOf"]),
        # :364-386
        schema_case("mined-repair-tuple-prefix-items",
                    obj({"prefix": {"type": "array",
                                    "prefixItems": [{"type": "integer"}, {"type": "boolean"}],
                                    "items": {"type": "number"}},
                         "draft": {"type": "array", "items": [{"type": "integer"}, {"type": "boolean"}]},
                         "valid": {"type": "array", "prefixItems": [{"type": "integer"}, {"type": "boolean"}]},
                         "scalar": {"type": "array", "prefixItems": [{"type": "integer"}]}}),
                    [{"prefix": ["2", "false", "3"], "draft": '["4","true"]',
                      "valid": [2, False], "scalar": "5"}],
                    ["type", "items", "prefixItems"]),
        # :388-429
        schema_case("mined-repair-refs-dictionary",
                    {**obj({"modern": {"$ref": "#/$defs/integer"},
                            "legacy": {"$ref": "#/definitions/boolean"},
                            "nested": {"$ref": "#/$defs/nested"},
                            "dictionary": {"type": "object",
                                           "additionalProperties": {"$ref": "#/$defs/integer"}},
                            "missing": {"$ref": "#/$defs/missing"},
                            "pointer": {"$ref": "#/$defs/nested/properties/count"},
                            "escaped": {"$ref": "#/$defs/a~1b~0c"}}),
                     "$defs": {"integer": {"type": "integer"},
                               "a/b~c": {"type": "integer"},
                               "nested": obj({"count": {"$ref": "#/$defs/integer"}})},
                     "definitions": {"boolean": {"type": "boolean"}}},
                    [{"modern": "2", "legacy": "false", "nested": {"count": "3"},
                      "dictionary": {"first": "4"}, "missing": "5", "pointer": "6",
                      "escaped": "7"}],
                    ["$ref", "$defs", "additionalProperties", "type"]),
        # :431-453
        schema_case("mined-repair-ambiguous-unions",
                    obj({"numeric": {"anyOf": [{"type": "number"}, {"type": "integer"}]},
                         "objects": {"oneOf": [obj({"value": {"type": "integer"}}, ["value"]),
                                               obj({"value": {"type": "number"}}, ["value"])]},
                         "both": {"anyOf": [{"type": "integer"}], "oneOf": [{"type": "integer"}]},
                         "composed": {"allOf": [{"type": "integer"}]},
                         "unknown": {}}),
                    [{"numeric": "2", "objects": {"value": "3"}, "both": "4",
                      "composed": "5", "unknown": "6"}],
                    ["anyOf", "oneOf", "allOf", "type", "required"]),
        schema_case("mined-repair-properties-only-root",
                    {"properties": {"numeric": {"type": "integer"}}},
                    [{"numeric": "2"}], ["properties"]),
        schema_case("mined-repair-allof-only-root",
                    {"allOf": [obj({"numeric": {"type": "integer"}})]},
                    [{"numeric": "2"}], ["allOf", "type"]),
    ]
    files.update(dict(cases))
    return write_files(CORPUS / "json_schema", files, "mined-*")


# ------------------------------------------------------------- ch5 compaction

CHECKPOINT = {"type": "compaction", "id": "cmp_1", "encrypted_content": "opaque"}
RETAINED = {"type": "message", "role": "user", "id": "msg_1", "status": "completed",
            "content": [{"type": "input_text", "text": "retained"}]}


def compaction_doc(output, usage=None, extra=None) -> str:
    doc = {"object": "response.compaction", "output": output}
    if usage:
        doc["usage"] = usage
    if extra:
        doc.update(extra)
    return json.dumps(doc, ensure_ascii=False, separators=(",", ":")) + "\n"


def mine_compaction() -> int:
    files = {}
    retained_items = [
        RETAINED,
        {"type": "message", "id": "msg_assistant", "role": "assistant", "status": "completed",
         "phase": "commentary",
         "content": [{"type": "output_text", "text": "First"}, {"type": "output_text", "text": "Second"}]},
        {"type": "reasoning", "id": "rs_1",
         "summary": [{"type": "summary_text", "text": "Thinking"},
                     {"type": "summary_text", "text": "More thinking"}],
         "encrypted_content": "reasoning-state"},
        {"type": "reasoning", "id": "rs_2", "summary": [], "encrypted_content": "hidden-reasoning"},
        {"type": "message", "id": "msg_media", "role": "user",
         "content": [
             {"type": "input_image", "image_url": "https://example.com/image.png"},
             {"type": "input_file", "filename": "report.pdf",
              "file_data": "data:application/pdf;base64,cGRm", "detail": "high"},
             {"type": "input_file", "filename": "other.pdf",
              "file_url": "https://example.com/report.pdf", "detail": "low"}]},
        CHECKPOINT,
    ]
    # reject-item family — explicit-compaction.test.ts:312-343
    reject_items = [
        {"type": "unknown_provider_item", "data": "do not hide in a compaction part"},
        {"type": "message", "role": "user",
         "content": [{"type": "input_image", "image_url": "https://example.com/image.png", "detail": 42}]},
        {"type": "message", "role": "user", "content": []},
        {"type": "message", "role": "assistant",
         "content": [{"type": "input_image", "image_url": "https://example.com/image.png"}]},
        {"type": "message", "role": "user", "content": [{"type": "input_file", "filename": "missing.pdf"}]},
        {"type": "message", "role": "user",
         "content": [{"type": "input_file", "filename": "bad.pdf",
                      "file_url": "https://example.com/report.pdf", "detail": 42}]},
        {"type": "message", "role": "user",
         "content": [{"type": "input_file", "filename": "both.pdf",
                      "file_url": "https://example.com/report.pdf",
                      "file_data": "data:application/pdf;base64,cGRm"}]},
    ]
    images = [{k: v for k, v in [("type", "input_image"),
                                 ("image_url", "https://example.com/image.png"),
                                 *([("detail", d)] if d else [])]}
              for d in (None, "low", "high", "auto")]
    docs = [
        ("mined-c01-retained-plus-checkpoint", compaction_doc([RETAINED, CHECKPOINT])),
        ("mined-c02-checkpoint-only", compaction_doc([CHECKPOINT])),
        ("mined-c03-retained-items-family", compaction_doc(retained_items)),
        ("mined-c04-image-detail-variants",
         compaction_doc([{"type": "message", "role": "user", "content": images}, CHECKPOINT])),
        ("mined-c05-output-usage",
         compaction_doc([RETAINED, CHECKPOINT],
                        usage={"input_tokens": 1000, "output_tokens": 10, "total_tokens": 1010})),
        ("mined-c06-extra-top-level-key",
         compaction_doc([RETAINED], extra={"debug": "original payload"})),
    ]
    for i, item in enumerate(reject_items, 1):
        docs.append((f"mined-r{i:02d}-reject-unsupported-item",
                     compaction_doc([item, CHECKPOINT])))
    files.update({f"{name}.json": text for name, text in docs})
    return write_files(CORPUS / "compaction", files, "mined-*")


# ------------------------------------------------------------------- ch6 http

HTTP_MINED = [
    # family L2-errors — error paths from ref packages/server/test/*.test.ts
    ("L2-errors", "vcs_uninit_get", "GET", "/api/vcs", None),                      # vcs.test.ts
    ("L2-errors", "vcs_base_uninit", "GET", "/api/vcs/base", None),                # vcs.test.ts:228
    ("L2-errors", "vcs_diff_uninit", "GET", "/api/vcs/diff", None),                # vcs.test.ts
    ("L2-errors", "vcs_init_missing_dir", "POST",
     "/api/vcs/init?location[directory]={WS}%2Fdiff-absent-dir", None),            # vcs.test.ts:74-84
    ("L2-errors", "vcs_init_unknown_provider", "POST",
     "/api/vcs/init?provider=unknown&location[directory]={WS}", None),             # vcs.test.ts:89-93
    ("L2-errors", "worktree_get_empty", "GET", "/api/worktree", None),             # worktree.test.ts:138
    ("L2-errors", "worktree_create_no_body", "POST", "/api/worktree", None),
    ("L2-errors", "fs_list_missing_dir", "GET",
     "/api/fs/list?location[directory]={WS}%2Fdiff-no-such", None),                # fs.test.ts:39
    ("L2-errors", "provider_missing", "GET", "/api/provider/does-not-exist", None),
    ("L2-errors", "session_missing_fork", "POST", "/api/session/{SID_MISSING}/fork", None),
    ("L2-errors", "config_patch_no_body", "PATCH", "/api/experimental/config", None),
    # family L3-side-effects
    ("L3-side-effects", "config_patch_shell", "PATCH", "/api/experimental/config",
     {"shell": "/bin/zsh"}),                                                       # config.test.ts:84
    ("L3-side-effects", "debug_location", "GET", "/api/debug/location", None),     # fs.test.ts:49
]


def mine_http() -> int:
    doc = {"_comment": "mined by tools/diff/mine_ref_fixtures.py from ref packages/server/test/*.test.ts",
           "requests": [{"family": f, "name": n, "method": m, "path": p, "body": b}
                        for f, n, m, p, b in HTTP_MINED]}
    out = CORPUS / "http"
    out.mkdir(parents=True, exist_ok=True)
    (out / "mined-http.json").write_text(json.dumps(doc, ensure_ascii=False, indent=1) + "\n")
    return len(HTTP_MINED)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ref", default="/opt/opencode-ref")
    ap.add_argument("--only", help="comma list: config,acp,sse,json_schema,compaction,http")
    args = ap.parse_args()
    ref = Path(args.ref)
    only = set(args.only.split(",")) if args.only else None

    counts = {}
    if not only or "config" in only:
        counts["config"] = mine_config(ref)
    if not only or "acp" in only:
        counts["acp"] = mine_acp()
    if not only or "sse" in only:
        counts["sse"] = mine_sse()
    if not only or "json_schema" in only:
        counts["json_schema"] = mine_json_schema()
    if not only or "compaction" in only:
        counts["compaction"] = mine_compaction()
    if not only or "http" in only:
        counts["http"] = mine_http()
    for k, v in counts.items():
        print(f"{k}: wrote {v} mined files")
    return 0


if __name__ == "__main__":
    sys.exit(main())
