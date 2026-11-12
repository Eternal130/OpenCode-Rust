#!/usr/bin/env python3
"""Generate the ACP-translate differential corpus (channel 2).

Writes one JSON case per file under tools/diff/corpus/acp/. Case shape:

  { "name": str, "ctx": {sessionID, cwd, start:{type,id}, childUpdates,
    compaction}, "events": [{id?, type, data}, ...], "abandon"?: bool }

Each case is fed event-by-event through both sides' pure `fold`
(ref packages/cli/src/acp/translate.ts vs rust oc-cli acp::translate).
The first event is always the inbox bootstrap that flips `started`.
Coverage: >=1 case for every one of the 51 session event types, plus
payload-level boundaries (empty/unicode/long deltas, null fields,
odd ordinals, multipart tool content, boolean fields, cancel family).
Idempotent: files are rewritten deterministically.
"""
from __future__ import annotations

import json
from pathlib import Path

OUT = Path(__file__).resolve().parent / "corpus" / "acp"
SID = "ses_root"
MSG = "msg_asst_1"
BOOT = {"id": "evt_boot", "type": "session.inbox.delivered",
        "data": {"sessionID": SID, "inboxID": "msg_inbox"}}

CTX0 = {"sessionID": SID, "cwd": "/tmp/opencode/diff-acp",
        "start": {"type": "input", "id": "msg_inbox"},
        "childUpdates": False, "compaction": True}


def ctx(**over):
    c = dict(CTX0)
    c.update(over)
    return c


def ev(type_, data, eid=None):
    e = {"type": type_, "data": data}
    if eid:
        e["id"] = eid
    return e


def case(name, events, context=None, abandon=False):
    rec = {"name": name, "ctx": context or CTX0,
           "events": [dict(BOOT)] + events}
    if abandon:
        rec["abandon"] = True
    return rec


TOKENS = {"input": 10, "output": 5, "reasoning": 2,
          "cache": {"read": 3, "write": 1}}


def main() -> None:
    cases = []

    # ---- one per event type (51) ------------------------------------
    # session.created is covered in the child section below; here the
    # "unrelated session" shape (parentID of another session).
    cases.append(case("created-unrelated-parent", [
        ev("session.created", {"sessionID": "ses_x", "projectID": "global",
                               "location": {"directory": "/repo"}, "slug": "s",
                               "parentID": "ses_other", "version": "2"},
          "evt_1")]))
    cases.append(case("created-no-parent", [
        ev("session.created", {"sessionID": "ses_x", "projectID": "global",
                               "location": {"directory": "/repo"}, "slug": "s",
                               "version": "2"}, "evt_1")]))
    cases.append(case("agent-selected", [
        ev("session.agent.selected", {"sessionID": SID, "agent": "build",
                                      "previous": "general"}, "evt_1")]))
    cases.append(case("model-selected", [
        ev("session.model.selected",
           {"sessionID": SID, "model": {"providerID": "anthropic",
                                        "modelID": "claude-3"},
            "previous": None}, "evt_1")]))
    cases.append(case("moved", [
        ev("session.moved", {"sessionID": SID, "projectID": "proj_2",
                             "location": {"directory": "/other"}},
          "evt_1")]))
    cases.append(case("renamed", [
        ev("session.renamed", {"sessionID": SID, "title": "新タイトル 🌍"},
          "evt_1")]))
    cases.append(case("metadata-updated", [
        ev("session.metadata.updated", {"sessionID": SID,
                                        "metadata": {"a": 1, "b": None}},
          "evt_1")]))
    cases.append(case("permissions", [
        ev("session.permissions", {"sessionID": SID, "permissions": {
            "allow": [{"tool": "bash"}], "deny": []}}, "evt_1")]))
    cases.append(case("viewed", [
        ev("session.viewed", {"sessionID": SID, "idle": 1731400000000},
          "evt_1")]))
    cases.append(case("usage-updated", [
        ev("session.usage.updated", {"sessionID": SID, "cost": 0.001,
                                     "tokens": TOKENS}, "evt_1")]))
    cases.append(case("usage-recorded", [
        ev("session.usage.recorded", {"sessionID": SID, "source": "title",
                                      "cost": 0.002, "tokens": TOKENS},
          "evt_1")]))
    cases.append(case("deleted", [
        ev("session.deleted", {"sessionID": SID}, "evt_1")]))
    cases.append(case("forked", [
        ev("session.forked", {"sessionID": SID, "parentID": "ses_old",
                              "boundary": "message",
                              "instructions": {}}, "evt_1")]))
    cases.append(case("inbox-enqueued", [
        ev("session.inbox.enqueued",
           {"sessionID": SID, "inboxID": "msg_q",
            "item": {"type": "message", "messageID": "msg_1"}}, "evt_1")]))
    cases.append(case("inbox-cancelled", [
        ev("session.inbox.cancelled", {"sessionID": SID, "inboxID": "msg_q"},
          "evt_1")]))
    cases.append(case("inbox-delivery-changed", [
        ev("session.inbox.delivery.changed",
           {"sessionID": SID, "inboxID": "msg_q", "delivery": "delivered"},
          "evt_1")]))
    cases.append(case("execution-started", [
        ev("session.execution.started", {"sessionID": SID}, "evt_1")]))
    cases.append(case("execution-succeeded-response", [
        ev("session.step.ended", {"sessionID": SID,
                                  "assistantMessageID": MSG, "finish": "stop",
                                  "cost": 0.001, "tokens": TOKENS}, "evt_1"),
        ev("session.execution.succeeded", {"sessionID": SID}, "evt_2")]))
    cases.append(case("execution-failed", [
        ev("session.execution.failed",
           {"sessionID": SID,
            "error": {"type": "provider.http", "message": "boom"}},
           "evt_1")]))
    cases.append(case("execution-interrupted", [
        ev("session.execution.interrupted", {"sessionID": SID,
                                             "reason": "user"}, "evt_1")]))
    cases.append(case("instructions-updated", [
        ev("session.instructions.updated",
           {"sessionID": SID, "delta": {}, "text": None}, "evt_1")]))
    cases.append(case("synthetic", [
        ev("session.synthetic", {"sessionID": SID, "text": "note",
                                 "description": None, "metadata": None},
          "evt_1")]))
    cases.append(case("skill-activated", [
        ev("session.skill.activated",
           {"sessionID": SID, "id": "skill_1", "name": "deploy",
            "text": "how to deploy"}, "evt_1")]))
    cases.append(case("shell-started", [
        ev("session.shell.started", {"sessionID": SID,
                                     "shell": {"id": "sh_1",
                                               "command": "ls"}},
          "evt_1")]))
    cases.append(case("shell-ended", [
        ev("session.shell.ended", {"sessionID": SID,
                                   "shell": {"id": "sh_1", "command": "ls"},
                                   "output": {"exit": 0, "stdout": ""}},
          "evt_1")]))
    cases.append(case("step-started", [
        ev("session.step.started",
           {"sessionID": SID, "assistantMessageID": MSG, "agent": "build",
            "model": {"providerID": "anthropic", "modelID": "claude-3"},
            "started": 0}, "evt_1")]))
    cases.append(case("step-streamed", [
        ev("session.step.streamed", {"sessionID": SID,
                                     "assistantMessageID": MSG}, "evt_1")]))
    cases.append(case("step-ended-usage", [
        ev("session.step.ended", {"sessionID": SID,
                                  "assistantMessageID": MSG, "finish": "stop",
                                  "cost": 0.001, "tokens": TOKENS}, "evt_1")]))
    cases.append(case("step-failed", [
        ev("session.step.failed",
           {"sessionID": SID, "assistantMessageID": MSG,
            "error": {"type": "provider.http", "message": "oops"},
            "tokens": None}, "evt_1")]))
    cases.append(case("text-started", [
        ev("session.text.started", {"sessionID": SID,
                                    "assistantMessageID": MSG, "ordinal": 0},
          "evt_1")]))
    cases.append(case("text-delta", [
        ev("session.text.delta", {"sessionID": SID,
                                  "assistantMessageID": MSG, "ordinal": 0,
                                  "delta": "hello"}, "evt_1")]))
    cases.append(case("text-ended", [
        ev("session.text.ended", {"sessionID": SID,
                                  "assistantMessageID": MSG, "ordinal": 0,
                                  "text": "hello", "state": None},
          "evt_1")]))
    cases.append(case("reasoning-started", [
        ev("session.reasoning.started",
           {"sessionID": SID, "assistantMessageID": MSG, "ordinal": 0,
            "state": None}, "evt_1")]))
    cases.append(case("reasoning-delta", [
        ev("session.reasoning.delta",
           {"sessionID": SID, "assistantMessageID": MSG, "ordinal": 0,
            "delta": "thinking"}, "evt_1")]))
    cases.append(case("reasoning-ended", [
        ev("session.reasoning.ended",
           {"sessionID": SID, "assistantMessageID": MSG, "ordinal": 0,
            "text": "thought", "state": None}, "evt_1")]))
    cases.append(case("tool-input-started", [
        ev("session.tool.input.started",
           {"sessionID": SID, "assistantMessageID": MSG, "id": "call_1",
            "name": "read"}, "evt_1")]))
    cases.append(case("tool-input-delta", [
        ev("session.tool.input.delta",
           {"sessionID": SID, "assistantMessageID": MSG, "id": "call_1",
            "delta": "{\"path\""}, "evt_1")]))
    cases.append(case("tool-input-ended", [
        ev("session.tool.input.ended",
           {"sessionID": SID, "assistantMessageID": MSG, "id": "call_1",
            "text": "{\"path\":\"a.ts\"}"}, "evt_1")]))
    cases.append(case("tool-called", [
        ev("session.tool.input.started",
           {"sessionID": SID, "assistantMessageID": MSG, "id": "call_1",
            "name": "shell"}, "evt_1"),
        ev("session.tool.called",
           {"sessionID": SID, "assistantMessageID": MSG, "id": "call_1",
            "input": {"command": "ls", "cwd": None}, "executed": False},
           "evt_2")]))
    cases.append(case("tool-progress", [
        ev("session.tool.input.started",
           {"sessionID": SID, "assistantMessageID": MSG, "id": "call_1",
            "name": "read"}, "evt_1"),
        ev("session.tool.progress",
           {"sessionID": SID, "assistantMessageID": MSG, "id": "call_1",
            "metadata": {"progress": 0.5}}, "evt_2")]))
    cases.append(case("tool-success", [
        ev("session.tool.input.started",
           {"sessionID": SID, "assistantMessageID": MSG, "id": "call_1",
            "name": "shell"}, "evt_1"),
        ev("session.tool.success",
           {"sessionID": SID, "assistantMessageID": MSG, "id": "call_1",
            "content": [{"type": "text", "text": "a.ts\nb.ts"}],
            "metadata": {"t": 1}, "executed": True}, "evt_2")]))
    cases.append(case("tool-failed", [
        ev("session.tool.input.started",
           {"sessionID": SID, "assistantMessageID": MSG, "id": "call_1",
            "name": "shell"}, "evt_1"),
        ev("session.tool.failed",
           {"sessionID": SID, "assistantMessageID": MSG, "id": "call_1",
            "error": {"type": "tool.failed", "message": "exit 1"},
            "content": None, "metadata": None, "executed": False},
           "evt_2")]))
    cases.append(case("retry-scheduled", [
        ev("session.retry.scheduled",
           {"sessionID": SID, "assistantMessageID": MSG, "attempt": 1,
            "at": 1731400000000,
            "error": {"type": "provider.http", "message": "5xx"}}, "evt_1")]))
    cases.append(case("retry-cleared-by-step-start", [
        ev("session.retry.scheduled",
           {"sessionID": SID, "assistantMessageID": MSG, "attempt": 1,
            "at": 1731400000000,
            "error": {"type": "provider.http", "message": "5xx"}}, "evt_1"),
        ev("session.step.started",
           {"sessionID": SID, "assistantMessageID": MSG, "agent": "build",
            "model": {"providerID": "a", "modelID": "m"}, "started": 0},
           "evt_2")]))
    cases.append(case("compaction-lifecycle", [
        ev("session.compaction.started",
           {"sessionID": SID, "reason": "manual", "recent": "msg_0",
            "inputID": "msg_cx"}, "evt_1"),
        ev("session.compaction.delta",
           {"sessionID": SID, "text": "summarising… 🌍"}, "evt_2"),
        ev("session.compaction.ended",
           {"sessionID": SID, "reason": "manual", "text": "the summary",
            "recent": "msg_0"}, "evt_3")]))
    cases.append(case("compaction-failed-auto", [
        ev("session.compaction.failed",
           {"sessionID": SID, "reason": "auto",
            "error": {"type": "provider.http", "message": "boom"},
            "inputID": "msg_cx"}, "evt_1")]))
    cases.append(case("compaction-marker-without-capability", [
        ev("session.compaction.started",
           {"sessionID": SID, "reason": "manual", "recent": "msg_0",
            "inputID": "msg_cx"}, "evt_1")],
        context=ctx(compaction=False)))
    cases.append(case("compaction-delta-without-started", [
        ev("session.compaction.delta", {"sessionID": SID, "text": "orphan"},
          "evt_1")]))
    cases.append(case("compaction-id-from-envelope", [
        # No data.inputID / data.id: ref derives msg_<n> from the
        # envelope event id, rust falls back to "" (gap candidate).
        ev("session.compaction.started",
           {"sessionID": SID, "reason": "manual", "recent": "msg_0"},
           "evt_7")]))
    cases.append(case("revert-staged", [
        ev("session.revert.staged",
           {"sessionID": SID, "revert": {"to": "msg_1",
                                         "files": {"a.ts": "pending"}}},
          "evt_1")]))
    cases.append(case("revert-cleared", [
        ev("session.revert.cleared", {"sessionID": SID}, "evt_1")]))
    cases.append(case("revert-committed", [
        ev("session.revert.committed", {"sessionID": SID, "to": "msg_1"},
          "evt_1")]))
    cases.append(case("message-content-updated", [
        ev("session.message.content.updated",
           {"sessionID": SID, "messageID": MSG,
            "content": [{"type": "text", "text": "edited"}]}, "evt_1")]))

    # ---- payload boundaries ------------------------------------------
    cases.append(case("text-delta-empty", [
        ev("session.text.delta", {"sessionID": SID,
                                  "assistantMessageID": MSG, "ordinal": 0,
                                  "delta": ""}, "evt_1")]))
    cases.append(case("text-delta-missing-field", [
        ev("session.text.delta", {"sessionID": SID,
                                  "assistantMessageID": MSG}, "evt_1")]))
    cases.append(case("text-delta-unicode-emoji", [
        ev("session.text.delta", {"sessionID": SID,
                                  "assistantMessageID": MSG, "ordinal": 0,
                                  "delta": "中文 🌍🚀 émoji 👨‍👩‍👧‍👦 \n\t"},
          "evt_1")]))
    cases.append(case("text-delta-long", [
        ev("session.text.delta", {"sessionID": SID,
                                  "assistantMessageID": MSG, "ordinal": 0,
                                  "delta": "x" * 50000}, "evt_1")]))
    cases.append(case("text-delta-null-session-id", [
        ev("session.text.delta", {"sessionID": None,
                                  "assistantMessageID": MSG, "ordinal": 0,
                                  "delta": "x"}, "evt_1")]))
    cases.append(case("text-delta-numeric-session-id", [
        ev("session.text.delta", {"sessionID": 123,
                                  "assistantMessageID": MSG, "ordinal": 0,
                                  "delta": "x"}, "evt_1")]))
    cases.append(case("reasoning-delta-ordinal-zero", [
        ev("session.reasoning.delta",
           {"sessionID": SID, "assistantMessageID": MSG, "ordinal": 0,
            "delta": "d"}, "evt_1")]))
    cases.append(case("reasoning-delta-ordinal-large", [
        ev("session.reasoning.delta",
           {"sessionID": SID, "assistantMessageID": MSG,
            "ordinal": 9007199254740991, "delta": "d"}, "evt_1")]))
    cases.append(case("reasoning-delta-ordinal-missing", [
        ev("session.reasoning.delta",
           {"sessionID": SID, "assistantMessageID": MSG, "delta": "d"},
          "evt_1")]))
    cases.append(case("reasoning-delta-ordinal-negative", [
        ev("session.reasoning.delta",
           {"sessionID": SID, "assistantMessageID": MSG, "ordinal": -1,
            "delta": "d"}, "evt_1")]))
    cases.append(case("reasoning-delta-ordinal-string", [
        ev("session.reasoning.delta",
           {"sessionID": SID, "assistantMessageID": MSG, "ordinal": "2",
            "delta": "d"}, "evt_1")]))
    cases.append(case("tool-lifecycle-shell", [
        ev("session.tool.input.started",
           {"sessionID": SID, "assistantMessageID": MSG, "id": "call_1",
            "name": "shell"}, "evt_1"),
        ev("session.tool.called",
           {"sessionID": SID, "assistantMessageID": MSG, "id": "call_1",
            "input": {"command": "ls -la"}, "executed": True}, "evt_2"),
        ev("session.tool.progress",
           {"sessionID": SID, "assistantMessageID": MSG, "id": "call_1",
            "metadata": {"p": 1}}, "evt_3"),
        ev("session.tool.success",
           {"sessionID": SID, "assistantMessageID": MSG, "id": "call_1",
            "content": [{"type": "text", "text": "total 0"}],
            "executed": True}, "evt_4")]))
    cases.append(case("tool-success-multipart-all-parts", [
        ev("session.tool.input.started",
           {"sessionID": SID, "assistantMessageID": MSG, "id": "call_1",
            "name": "screenshot"}, "evt_1"),
        ev("session.tool.success",
           {"sessionID": SID, "assistantMessageID": MSG, "id": "call_1",
            "content": [
                {"type": "text", "text": "captured"},
                {"type": "file", "uri": "data:image/png;base64,aGVsbG8=",
                 "mime": "image/png"},
                {"type": "file", "uri": "data:application/json;base64,e30=",
                 "mime": "application/json", "name": "out.json"},
                {"type": "file", "uri": "file:///tmp/opencode/x.txt",
                 "mime": "text/plain"},
            ], "executed": True}, "evt_2")]))
    cases.append(case("tool-success-read-preview-g339", [
        # G-339: read's text-page envelope preview.
        ev("session.tool.input.started",
           {"sessionID": SID, "assistantMessageID": MSG, "id": "call_1",
            "name": "read"}, "evt_1"),
        ev("session.tool.called",
           {"sessionID": SID, "assistantMessageID": MSG, "id": "call_1",
            "input": {"path": "src/a.ts"}, "executed": True}, "evt_2"),
        ev("session.tool.success",
           {"sessionID": SID, "assistantMessageID": MSG, "id": "call_1",
            "content": [{"type": "text",
                         "text": "{\"type\":\"text-page\",\"content\":\"1 let a = 1\\n2 let b = 2\"}"}],
            "executed": True}, "evt_3")]))
    cases.append(case("tool-success-edit-diff-content", [
        # completedToolUpdate's oldString/newString diff branch.
        ev("session.tool.input.started",
           {"sessionID": SID, "assistantMessageID": MSG, "id": "call_1",
            "name": "edit"}, "evt_1"),
        ev("session.tool.called",
           {"sessionID": SID, "assistantMessageID": MSG, "id": "call_1",
            "input": {"path": "src/a.ts", "oldString": "a", "newString": "b"},
            "executed": True}, "evt_2"),
        ev("session.tool.success",
           {"sessionID": SID, "assistantMessageID": MSG, "id": "call_1",
            "content": [{"type": "text", "text": "applied"}],
            "executed": True}, "evt_3")]))
    cases.append(case("tool-called-without-started-fallback-tool", [
        ev("session.tool.called",
           {"sessionID": SID, "assistantMessageID": MSG, "id": "call_9",
            "input": {"x": 1}, "executed": False}, "evt_1")]))
    cases.append(case("tool-success-without-started", [
        ev("session.tool.success",
           {"sessionID": SID, "assistantMessageID": MSG, "id": "call_9",
            "content": [{"type": "text", "text": "ok"}],
            "executed": True}, "evt_1")]))
    cases.append(case("tool-progress-without-started", [
        ev("session.tool.progress",
           {"sessionID": SID, "assistantMessageID": MSG, "id": "call_9",
            "metadata": {}}, "evt_1")]))
    cases.append(case("tool-failed-long-message", [
        ev("session.tool.input.started",
           {"sessionID": SID, "assistantMessageID": MSG, "id": "call_1",
            "name": "shell"}, "evt_1"),
        ev("session.tool.failed",
           {"sessionID": SID, "assistantMessageID": MSG, "id": "call_1",
            "error": {"type": "tool.failed", "message": "E" * 4000},
            "content": [{"type": "text", "text": "partial"}],
            "executed": False}, "evt_2")]))
    cases.append(case("retry-scheduled-at-zero", [
        ev("session.retry.scheduled",
           {"sessionID": SID, "assistantMessageID": MSG, "attempt": 1,
            "at": 0, "error": {"type": "t", "message": "m"}}, "evt_1")]))
    cases.append(case("retry-then-interrupt-response-meta-g340", [
        ev("session.retry.scheduled",
           {"sessionID": SID, "assistantMessageID": MSG, "attempt": 2,
            "at": 1731400060123,
            "error": {"type": "provider.http", "message": "5xx"}}, "evt_1"),
        ev("session.execution.interrupted", {"sessionID": SID,
                                             "reason": "user"}, "evt_2")]))

    # ---- terminal/stop-reason matrix ---------------------------------
    cases.append(case("response-max-tokens", [
        ev("session.step.ended", {"sessionID": SID,
                                  "assistantMessageID": MSG, "finish": "length",
                                  "cost": 0, "tokens": TOKENS}, "evt_1"),
        ev("session.execution.succeeded", {"sessionID": SID}, "evt_2")]))
    cases.append(case("response-refusal-finish", [
        ev("session.step.ended", {"sessionID": SID,
                                  "assistantMessageID": MSG,
                                  "finish": "content-filter", "cost": 0,
                                  "tokens": TOKENS}, "evt_1"),
        ev("session.execution.succeeded", {"sessionID": SID}, "evt_2")]))
    cases.append(case("response-refusal-error", [
        ev("session.step.failed",
           {"sessionID": SID, "assistantMessageID": MSG,
            "error": {"type": "provider.content-filter", "message": "no"}},
           "evt_1"),
        ev("session.execution.succeeded", {"sessionID": SID}, "evt_2")]))
    cases.append(case("response-cancelled-aborted", [
        ev("session.step.failed",
           {"sessionID": SID, "assistantMessageID": MSG,
            "error": {"type": "aborted", "message": "cancelled"},
            "tokens": TOKENS}, "evt_1"),
        ev("session.execution.succeeded", {"sessionID": SID}, "evt_2")]))
    cases.append(case("response-usage-accumulates", [
        ev("session.step.ended", {"sessionID": SID,
                                  "assistantMessageID": MSG, "finish": "stop",
                                  "cost": 0, "tokens": TOKENS}, "evt_1"),
        ev("session.step.ended", {"sessionID": SID,
                                  "assistantMessageID": MSG, "finish": "stop",
                                  "cost": 0,
                                  "tokens": {"input": 1, "output": 2,
                                             "reasoning": 0,
                                             "cache": {"read": 0,
                                                       "write": 0}}}, "evt_2"),
        ev("session.execution.succeeded", {"sessionID": SID}, "evt_3")]))
    cases.append(case("response-failed-terminal-auth", [
        ev("session.execution.failed",
           {"sessionID": SID,
            "error": {"type": "provider.auth", "message": "expired"}},
           "evt_1")]))

    # ---- ask / settle family -----------------------------------------
    cases.append(case("permission-ask-tracked-tool", [
        ev("session.tool.input.started",
           {"sessionID": SID, "assistantMessageID": MSG, "id": "call_1",
            "name": "shell"}, "evt_1"),
        ev("permission.asked",
           {"id": "perm_1", "sessionID": SID, "source": {"id": "call_1"},
            "tool": "bash"}, "evt_2"),
        ev("permission.replied", {"requestID": "perm_1"}, "evt_3")]))
    cases.append(case("permission-ask-untracked", [
        ev("permission.asked",
           {"id": "perm_2", "sessionID": SID}, "evt_1")]))
    cases.append(case("permission-replied-without-ask", [
        ev("permission.replied", {"requestID": "perm_x"}, "evt_1")]))
    cases.append(case("form-ask-and-settle", [
        ev("form.created",
           {"form": {"id": "form_1", "sessionID": SID, "title": "T"}},
           "evt_1"),
        ev("form.replied", {"id": "form_1"}, "evt_2"),
        ev("form.created",
           {"form": {"id": "form_2", "sessionID": SID, "title": "T2"}},
           "evt_3"),
        ev("form.cancelled", {"id": "form_2"}, "evt_4")]))
    cases.append(case("form-replied-without-ask", [
        ev("form.replied", {"id": "form_x"}, "evt_1")]))
    cases.append(case("permission-ask-other-session-ignored", [
        ev("permission.asked",
           {"id": "perm_3", "sessionID": "ses_other"}, "evt_1")]))

    # ---- child sessions ----------------------------------------------
    cases.append(case("child-projection-meta", [
        ev("session.created",
           {"sessionID": "ses_child1", "projectID": "global",
            "location": {"directory": "/repo"}, "slug": "s",
            "parentID": SID, "title": "Helper", "version": "2"}, "evt_1"),
        ev("session.text.delta",
           {"sessionID": "ses_child1", "assistantMessageID": "msg_c1",
            "ordinal": 0, "delta": "child says hi"}, "evt_2"),
        ev("session.tool.input.started",
           {"sessionID": "ses_child1", "assistantMessageID": "msg_c1",
            "id": "call_c", "name": "read"}, "evt_3")]))
    cases.append(case("child-updates-notifications", [
        ev("session.created",
           {"sessionID": "ses_child1", "projectID": "global",
            "location": {"directory": "/repo"}, "slug": "s",
            "parentID": SID, "title": "Helper", "version": "2"}, "evt_1"),
        ev("session.execution.started", {"sessionID": "ses_child1"},
          "evt_2"),
        ev("session.text.delta",
           {"sessionID": "ses_child1", "assistantMessageID": "msg_c1",
            "ordinal": 0, "delta": "x"}, "evt_3"),
        ev("session.execution.succeeded", {"sessionID": "ses_child1"},
          "evt_4")], context=ctx(childUpdates=True)))
    cases.append(case("child-nested-depth2", [
        ev("session.created",
           {"sessionID": "ses_child1", "projectID": "global",
            "location": {"directory": "/repo"}, "slug": "s",
            "parentID": SID, "version": "2"}, "evt_1"),
        ev("session.created",
           {"sessionID": "ses_child2", "projectID": "global",
            "location": {"directory": "/repo"}, "slug": "s",
            "parentID": "ses_child1", "title": "Deep", "version": "2"},
           "evt_2"),
        ev("session.text.delta",
           {"sessionID": "ses_child2", "assistantMessageID": "msg_c2",
            "ordinal": 0, "delta": "nested"}, "evt_3")],
        context=ctx(childUpdates=True)))
    cases.append(case("child-execution-failed-status", [
        ev("session.created",
           {"sessionID": "ses_child1", "projectID": "global",
            "location": {"directory": "/repo"}, "slug": "s",
            "parentID": SID, "version": "2"}, "evt_1"),
        ev("session.execution.failed",
           {"sessionID": "ses_child1",
            "error": {"type": "provider.http", "message": "down"}},
           "evt_2")], context=ctx(childUpdates=True)))
    cases.append(case("child-compaction-marker", [
        # Child + compaction=true + childUpdates=false → standard
        # updates suppressed, marker _meta instead.
        ev("session.created",
           {"sessionID": "ses_child1", "projectID": "global",
            "location": {"directory": "/repo"}, "slug": "s",
            "parentID": SID, "version": "2"}, "evt_1"),
        ev("session.compaction.started",
           {"sessionID": "ses_child1", "reason": "auto", "recent": "m",
            "inputID": "msg_cx"}, "evt_2")]))

    # ---- abandon (cancel family, G-354/G-355) ------------------------
    cases.append(case("abandon-tools-and-compaction", [
        ev("session.tool.input.started",
           {"sessionID": SID, "assistantMessageID": MSG, "id": "call_1",
            "name": "shell"}, "evt_1"),
        ev("session.tool.called",
           {"sessionID": SID, "assistantMessageID": MSG, "id": "call_1",
            "input": {"command": "sleep 100"}, "executed": True}, "evt_2"),
        ev("session.compaction.started",
           {"sessionID": SID, "reason": "manual", "recent": "m",
            "inputID": "msg_cx"}, "evt_3")], abandon=True))
    cases.append(case("abandon-empty-state", [], abandon=True))
    cases.append(case("abandon-child-tool", [
        ev("session.created",
           {"sessionID": "ses_child1", "projectID": "global",
            "location": {"directory": "/repo"}, "slug": "s",
            "parentID": SID, "version": "2"}, "evt_1"),
        ev("session.tool.input.started",
           {"sessionID": "ses_child1", "assistantMessageID": "msg_c1",
            "id": "call_c", "name": "read"}, "evt_2")], abandon=True))

    # ---- misc gating --------------------------------------------------
    cases.append(case("events-before-start-ignored", [
        {"type": "session.text.delta",
         "data": {"sessionID": SID, "assistantMessageID": MSG, "ordinal": 0,
                  "delta": "too early"}},
        {"id": "evt_late", "type": "session.inbox.delivered",
         "data": {"sessionID": SID, "inboxID": "msg_inbox"}},
        ev("session.text.delta", {"sessionID": SID,
                                  "assistantMessageID": MSG, "ordinal": 0,
                                  "delta": "in time"}, "evt_2")]))
    cases.append(case("inbox-delivered-wrong-id-ignored", [
        {"id": "evt_w", "type": "session.inbox.delivered",
         "data": {"sessionID": SID, "inboxID": "msg_other"}},
        ev("session.text.delta", {"sessionID": SID,
                                  "assistantMessageID": MSG, "ordinal": 0,
                                  "delta": "still gated"}, "evt_1")]))
    cases.append(case("other-session-ignored", [
        ev("session.text.delta", {"sessionID": "ses_other",
                                  "assistantMessageID": MSG, "ordinal": 0,
                                  "delta": "not mine"}, "evt_1")]))

    OUT.mkdir(parents=True, exist_ok=True)
    for old in OUT.glob("*.json"):
        old.unlink()
    for i, rec in enumerate(cases):
        (OUT / f"{i:03d}-{rec['name']}.json").write_text(
            json.dumps(rec, ensure_ascii=False, indent=1) + "\n")
    print(f"wrote {len(cases)} cases -> {OUT}")


if __name__ == "__main__":
    main()
