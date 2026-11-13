#!/usr/bin/env python3
"""Provider protocol-decode differential (channel 7 of the diff layer).

Corpus: the ref HttpRecorder cassettes
(/opt/opencode-ref/packages/ai/test/fixtures/recordings, same-source copy at
crates/oc-ai/test-fixtures/recordings).  The RECORDED RESPONSE BODY of every
LLM-protocol interaction (SSE text or JSON) is fed to BOTH sides' protocol
decoders and the decoded LLMEvent streams are diffed after normalization.
The request half of each cassette is NOT decoded; it is only mined for
harness context (provider/model id) that the decoders' initial state needs.

  ref  : bun  tools/diff/drivers/provider_recordings_ref.ts
         (staged to /tmp/opencode/diff-provrec with node_modules ->
          $REF/packages/ai/node_modules).  Drives the REAL protocol step
         machines (packages/ai/src/protocols/*.ts: stream.event decode +
         stream.initial + stream.step + terminal/onHalt) over the recorded
         body with the family's real SSE framing.
  rust : cargo nextest -p oc-ai --test provider_recordings_diff_driver
         (env-gated: PROVIDER_DIFF_MANIFEST/PROVIDER_DIFF_OUT).  Replays the
         body through the production oc-ai routes (MockTransport +
         route.stream), i.e. the same entry the cassette parity tests use.

Normalizations (declared, applied identically to both sides):
  - recursive key sorting (key order never counts);
  - local part ids stripped on text-*/reasoning-* events (rust/ref derive
    their own `text-0` style ids; tool-call ids are wire data and kept);
  - finish `reason.normalized`/`reason.raw` lowercased;
  - numbers compared as values (0 == 0.0);
  - an interaction that ends in a decoder error compares as ok=false plus
    the event prefix emitted before the error (error message text itself is
    NOT compared - both-sides-errored = MATCH, one-side = DIVERGE).

Out of scope (declared in the report with counts): binary AWS event-stream
(bedrock-converse), websocket channel (openai-responses-websocket), all
media/speech/transcription/image/video families, evaluation fixtures, and
the gateway-wrapped protocols (vercel-native, cloudflare-ai-gateway) whose
step machines are wrapped by gateway usage-merge logic outside this
channel's decode-only seam.

Output: markdown report (default docs/audit/diff/PROVIDER-RECORDINGS.md).
Verdicts per interaction: MATCH / DIVERGE.  Exit 0 - a census/diff report,
not a gate.  Deterministic: sorted cases, canonical JSON.
"""
from __future__ import annotations

import argparse
import base64
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent
REF_DEFAULT = Path("/opt/opencode-ref")
STAGE = Path("/tmp/opencode/diff-provrec")

# ---------------------------------------------------------------------------
# Family routing table.
#
# family: protocol family (decode machine shared by the group)
# provider: ref ProviderID used to build the model (decode-visible only via
#     metadata keying); meta_key: ref providerMetadataKey of the group's route
# compat: ref model compatibility (from the ref provider sources; the rust
#     side uses its own ported `*_chat_compatibility` fns keyed by group)
# url: optional substring filter - mixed cassettes route per interaction
# framing: sse | sse_done | sse_events (the group's real wire framing)
# ---------------------------------------------------------------------------
F = "family provider meta_key compat framing url".split()

GROUPS = [
    # anthropic-messages family
    dict(family="anthropic-messages", group="anthropic-messages", provider="anthropic", meta_key="anthropic", compat=None, framing="sse_events"),
    dict(family="anthropic-messages", group="anthropic-messages-cache", provider="anthropic", meta_key="anthropic", compat=None, framing="sse_events"),
    dict(family="anthropic-messages", group="anthropic-compatible-messages", provider="anthropic-compatible", meta_key="anthropic", compat=None, framing="sse_events"),
    dict(family="anthropic-messages", group="alibaba-messages", provider="alibaba", meta_key="alibaba", compat={"requireSignature": False}, framing="sse_events"),
    dict(family="anthropic-messages", group="moonshot-messages", provider="moonshotai", meta_key="moonshot", compat={"requireSignature": False}, framing="sse_events"),
    dict(family="anthropic-messages", group="minimax-messages", provider="minimax", meta_key="minimax", compat=None, framing="sse_events"),
    dict(family="anthropic-messages", group="meta-messages", provider="meta", meta_key="meta", compat={"requireSignature": False}, framing="sse_events"),
    dict(family="anthropic-messages", group="bedrock-mantle-messages", provider="bedrock-mantle", meta_key="mantle", compat=None, framing="sse_events"),
    dict(family="anthropic-messages", group="pdf", provider="anthropic", meta_key="anthropic", compat=None, framing="sse_events", url="/v1/messages"),
    dict(family="anthropic-messages", group="meta-search", provider="meta", meta_key="meta", compat={"requireSignature": False}, framing="sse_events", url="/v1/messages"),
    # openai responses (openai-hosted dialect: hosted tools + channel flags)
    dict(family="openai-responses", group="openai-responses", provider="openai", meta_key="openai", compat=None, framing="sse"),
    dict(family="openai-responses", group="openai-responses-cache", provider="openai", meta_key="openai", compat=None, framing="sse"),
    dict(family="openai-responses", group="openai-responses-images", provider="openai", meta_key="openai", compat=None, framing="sse"),
    dict(family="openai-responses", group="azure", provider="azure", meta_key="azure", compat=None, framing="sse", url="/responses"),
    dict(family="openai-responses", group="pdf", provider="openai", meta_key="openai", compat=None, framing="sse", url="api.openai.com/v1/responses"),
    dict(family="xai-responses", group="pdf", provider="xai", meta_key="xai", compat=None, framing="sse", url="api.x.ai/v1/responses"),
    # open responses (provider-neutral baseline)
    dict(family="open-responses", group="moonshot-responses", provider="moonshotai", meta_key="moonshot", compat=None, framing="sse"),
    dict(family="open-responses", group="minimax-responses", provider="minimax", meta_key="minimax", compat=None, framing="sse"),
    dict(family="open-responses", group="alibaba-responses", provider="alibaba", meta_key="alibaba", compat=None, framing="sse"),
    dict(family="open-responses", group="bedrock-mantle", provider="bedrock-mantle", meta_key="mantle", compat=None, framing="sse"),
    dict(family="open-responses", group="meta-search", provider="meta", meta_key="meta", compat=None, framing="sse", url="/v1/responses"),
    # meta responses (completedItems tracking dialect)
    dict(family="meta-responses", group="meta-responses", provider="meta", meta_key="meta", compat=None, framing="sse"),
    # openai chat family
    dict(family="openai-chat", group="groq-chat", provider="groq", meta_key="groq", compat={"maxTokensField": "max_completion_tokens", "reasoningField": "reasoning", "requireReasoning": False, "supportsStore": False, "supportsStrictMode": False}, framing="sse_done"),
    dict(family="openai-chat", group="cerebras-chat", provider="cerebras", meta_key="cerebras", compat={"maxTokensField": "max_tokens", "reasoningField": "reasoning", "supportsStore": False, "supportsPromptCacheKey": True}, framing="sse_done"),
    dict(family="openai-chat", group="deepinfra-chat", provider="deepinfra", meta_key="deepinfra", compat={"maxTokensField": "max_tokens", "reasoningField": "reasoning_content", "supportsStore": False, "supportsPromptCacheKey": True}, framing="sse_done"),
    dict(family="openai-chat", group="digitalocean-chat", provider="digitalocean", meta_key="digitalocean", compat={"supportsPromptCacheKey": True}, framing="sse_done"),
    dict(family="openai-chat", group="meta-chat", provider="meta", meta_key="meta", compat={"maxTokensField": "max_completion_tokens", "supportsStore": False, "supportsPromptCacheKey": True}, framing="sse_done"),
    dict(family="openai-chat", group="minimax-chat", provider="minimax", meta_key="minimax", compat={"supportsStore": False, "supportsStrictMode": False}, framing="sse_done"),
    dict(family="openai-chat", group="moonshot-chat", provider="moonshotai", meta_key="moonshot", compat={"maxTokensField": "max_tokens", "supportsStore": False, "supportsStrictMode": False, "supportsPromptCacheKey": True, "reasoningField": "reasoning_content"}, framing="sse_done"),
    dict(family="openai-chat", group="openai-compatible-chat", provider="openai-compatible", meta_key="openai-compatible", compat=None, framing="sse_done"),
    dict(family="openai-chat", group="venice-chat", provider="venice", meta_key="venice", compat={"maxTokensField": "max_completion_tokens", "supportsStore": False, "supportsPromptCacheKey": True}, framing="sse_done"),
    dict(family="openai-chat", group="zai-chat", provider="zai", meta_key="zai", compat={"maxTokensField": "max_tokens", "supportsStore": False, "supportsStrictMode": False, "reasoningField": "reasoning_content", "zaiToolStream": False}, framing="sse_done"),
    dict(family="openai-chat", group="alibaba-chat", provider="alibaba", meta_key="alibaba", compat={"maxTokensField": "max_completion_tokens", "supportsStore": False, "supportsStrictMode": False, "reasoningField": "reasoning_content", "zaiToolStream": False}, framing="sse"),
    dict(family="openai-chat", group="cloudflare-workers-ai", provider="cloudflare-workers-ai", meta_key="workers-ai", compat=None, framing="sse_done"),
    dict(family="openai-chat", group="azure", provider="azure", meta_key="azure", compat=None, framing="sse_done", url="/chat/completions"),
    dict(family="openai-chat", group="cohere", provider="cohere", meta_key="cohere", compat={"maxTokensField": "max_tokens", "supportsStore": False, "supportsUsageInStreaming": True, "reasoningField": "reasoning_content", "supportsStrictMode": False}, framing="sse_done", url="compatibility/v1/chat"),
    # mistral chat (own wire dialect)
    dict(family="mistral-chat", group="mistral-chat", provider="mistral", meta_key="mistral", compat=None, framing="sse_done"),
    dict(family="mistral-chat", group="mistral-chat-glm", provider="mistral", meta_key="mistral", compat=None, framing="sse_done"),
    # cohere native v2/chat
    dict(family="cohere-chat", group="cohere", provider="cohere", meta_key="cohere", compat=None, framing="sse", url="/v2/chat"),
    # gemini generateContent (google + vertex-hosted)
    dict(family="gemini", group="gemini", provider="google", meta_key="google", compat=None, framing="sse"),
    dict(family="gemini", group="gemini-cache", provider="google", meta_key="google", compat=None, framing="sse"),
    dict(family="gemini", group="google-vertex", provider="google-vertex", meta_key="vertex", compat=None, framing="sse"),
    dict(family="gemini", group="pdf", provider="google", meta_key="google", compat=None, framing="sse", url="generativelanguage"),
    # google interactions (native)
    dict(family="google-interactions", group="google-interactions", provider="google", meta_key="google", compat=None, framing="sse"),
]

# Out-of-scope cassette GROUPS (whole directory) with reasons; per-interaction
# exclusions inside driven mixed groups (e.g. pdf bedrock-converse) are
# counted as out-of-scope interactions.
OUT_GROUPS = {
    "bedrock-converse": "binary AWS event-stream framing (not text SSE/JSON decode)",
    "bedrock-converse-cache": "binary AWS event-stream framing",
    "bedrock-converse-thinking-binding": "binary AWS event-stream framing",
    "openai-responses-websocket": "websocket channel transport",
    "vercel-native": "gateway-wrapped step machine (usage/cost merge seam)",
    "cloudflare-ai-gateway": "gateway-wrapped provider translation",
    "opencode-zen-evaluation": "evaluation fixture (not a protocol decode lane)",
    "openrouter-evaluation": "evaluation fixture",
    "typesafe-evaluation": "evaluation fixture",
}


def group_of(path: Path) -> str:
    return path.parent.name


def model_of(meta: dict, request_body: str | None) -> str:
    if meta.get("model"):
        return str(meta["model"])
    if request_body:
        try:
            value = json.loads(request_body)
            if isinstance(value, dict) and value.get("model"):
                return str(value["model"])
        except Exception:
            pass
    return "test-model"


def build_manifest(ref: Path, out_dir: Path) -> tuple[list[dict], dict]:
    rec_root = ref / "packages/ai/test/fixtures/recordings"
    bodies = out_dir / "bodies"
    bodies.mkdir(parents=True, exist_ok=True)
    by_group: dict[str, dict] = {g["group"]: g for g in GROUPS}
    # a group may appear in several table rows (mixed cassettes)
    rows_of: dict[str, list[dict]] = {}
    for g in GROUPS:
        rows_of.setdefault(g["group"], []).append(g)

    cases: list[dict] = []
    census: dict[str, dict] = {}
    for group in sorted(os.listdir(rec_root)):
        gdir = rec_root / group
        if not gdir.is_dir():
            continue  # loose top-level fixture files (non-cassette)
        stats = census.setdefault(group, {"cassettes": 0, "in": 0, "out": 0, "reason": ""})
        rows = rows_of.get(group)
        reason = None if rows else OUT_GROUPS.get(group, "no LLM protocol decoder mapping for this channel")
        for fname in sorted(os.listdir(gdir)):
            if not fname.endswith(".json"):
                continue
            stats["cassettes"] += 1
            doc = json.loads((gdir / fname).read_text())
            meta = doc.get("metadata") or {}
            for idx, interaction in enumerate(doc.get("interactions", [])):
                request = interaction.get("request") or {}
                response = interaction.get("response") or {}
                url = str(request.get("url") or "")
                row = None
                if rows:
                    if len(rows) == 1:
                        row = rows[0]
                    else:
                        for candidate in rows:
                            if not candidate.get("url") or candidate["url"] in url:
                                row = candidate
                                break
                if row is None and reason is None and rows:
                    # mixed cassette interaction with no matching lane
                    reason_i = "interaction endpoint not in this channel's family table (e.g. bedrock binary / media)"
                    stats["out"] += 1
                    stats["reason"] = stats["reason"] or reason_i
                    continue
                if row is None:
                    stats["out"] += 1
                    stats["reason"] = reason
                    continue
                if interaction.get("transport") == "websocket":
                    stats["out"] += 1
                    stats["reason"] = "websocket transport"
                    continue
                body = response.get("body")
                if body is None:
                    stats["out"] += 1
                    stats["reason"] = stats["reason"] or "no recorded response body"
                    continue
                if response.get("bodyEncoding") == "base64":
                    body = base64.b64decode(body).decode("utf-8", errors="replace")
                req_body = request.get("body")
                if isinstance(req_body, str):
                    req_body_text = req_body
                else:
                    req_body_text = None
                name = f"{group}/{fname[:-5]}#{idx}"
                body_file = bodies / f"{len(cases):04d}.txt"
                body_file.write_text(body)
                cases.append({
                    "name": name,
                    "family": row["family"],
                    "group": group,
                    "provider": row["provider"],
                    "meta_key": row["meta_key"],
                    "compat": row["compat"],
                    "model": model_of(meta, req_body_text),
                    "framing": row["framing"],
                    "status": int(response.get("status") or 200),
                    "content_type": (response.get("headers") or {}).get("content-type", ""),
                    "url": url,
                    "body_file": str(body_file),
                })
                stats["in"] += 1
    cases.sort(key=lambda c: c["name"])
    return cases, census


def run_ref(stage: Path, ref: Path, manifest: Path, out: Path) -> None:
    stage.mkdir(parents=True, exist_ok=True)
    staged = stage / "provider_recordings_ref_driver.ts"
    shutil.copyfile(REPO / "tools/diff/drivers/provider_recordings_ref.ts", staged)
    nm = stage / "node_modules"
    if not nm.exists():
        nm.symlink_to(ref / "packages/ai/node_modules")
    env = {**os.environ, "DIFF_REF_ROOT": str(ref)}
    res = subprocess.run(
        ["bun", str(staged), "--manifest", str(manifest), "--out", str(out)],
        cwd=stage, env=env, capture_output=True, text=True,
    )
    if res.returncode != 0:
        print(res.stdout[-4000:], res.stderr[-4000:], sep="\n", file=sys.stderr)
        raise SystemExit(f"ref driver failed: rc={res.returncode}")


def run_rust(manifest: Path, out: Path) -> None:
    env = {**os.environ, "PROVIDER_DIFF_MANIFEST": str(manifest), "PROVIDER_DIFF_OUT": str(out)}
    res = subprocess.run(
        ["cargo", "nextest", "run", "-p", "oc-ai", "--test", "provider_recordings_diff_driver"],
        cwd=REPO, env=env, capture_output=True, text=True,
    )
    if res.returncode != 0 or not out.exists():
        print(res.stdout[-4000:], res.stderr[-4000:], sep="\n", file=sys.stderr)
        raise SystemExit(f"rust driver failed: rc={res.returncode}")


# ---------------------------------------------------------------------------
# normalization + diff
# ---------------------------------------------------------------------------

LOCAL_ID_EVENTS = {
    "text-start", "text-delta", "text-end",
    "reasoning-start", "reasoning-delta", "reasoning-end",
}


def canon(value):
    if isinstance(value, dict):
        return {k: canon(value[k]) for k in sorted(value)}
    if isinstance(value, list):
        return [canon(v) for v in value]
    if isinstance(value, float) and value == int(value):
        return int(value)
    return value


def norm_event(event: dict) -> dict:
    event = dict(event)
    kind = event.get("type")
    if kind in LOCAL_ID_EVENTS:
        event.pop("id", None)
    if kind == "finish":
        reason = event.get("reason")
        if isinstance(reason, dict):
            reason = dict(reason)
            for key in ("normalized", "raw"):
                if isinstance(reason.get(key), str):
                    reason[key] = reason[key].lower()
            event["reason"] = reason
    return canon(event)


def norm_case(rec: dict) -> dict:
    events = [norm_event(e) for e in rec.get("events", [])]
    return {
        "ok": bool(rec.get("ok")),
        "events": events,
        # error text/classification is harness-noisy: only ok-ness + event
        # prefix are compared (both-sides-errored = MATCH)
        "error": bool(rec.get("error")),
    }


def first_diff(a, b, path="$"):
    if isinstance(a, dict) and isinstance(b, dict):
        for k in sorted(set(a) | set(b)):
            if k not in a:
                return f"{path}.{k}: missing on rust side"
            if k not in b:
                return f"{path}.{k}: missing on ref side"
            d = first_diff(a[k], b[k], f"{path}.{k}")
            if d:
                return d
        return None
    if isinstance(a, list) and isinstance(b, list):
        if len(a) != len(b):
            return f"{path}: length {len(a)} (rust) != {len(b)} (ref)"
        for i, (x, y) in enumerate(zip(a, b)):
            d = first_diff(x, y, f"{path}[{i}]")
            if d:
                return d
        return None
    if a != b:
        return f"{path}: rust={json.dumps(a)[:160]} ref={json.dumps(b)[:160]}"
    return None


# Known-cause annotations (dual file:line) for divergence clusters; anything
# not covered here is an unadjudicated candidate.
CAUSES = [
    (
        "google-interactions 空 providerMetadata 命名空间",
        lambda scope, d: "providerMetadata: missing on rust side" in d and "google-interactions" in scope,
        (
            "ref 在 reasoning-start 等事件上无条件附着 providerMetadata 命名空间对象"
            "（metadata(state,step) 无内容时为 {google:{}} 空对象）；rust 仅在存在实际字段"
            "（如 thoughtSignature）时附着——空命名空间对象的附着语义差异",
            "ref packages/ai/src/protocols/google-interactions.ts:291,377",
            "rust crates/oc-ai/src/protocols/google_interactions.rs（事件投影按内容条件附着）",
        ),
    ),
    (
        "venice 空 providerMetadata.venice 命名空间",
        lambda scope, d: "providerMetadata: missing on ref side" in d and "venice-chat" in scope and "__ENCRYPTED_REASONING__" not in d,
        (
            "与 google-interactions 相反方向: rust 在 tool-input-end/reasoning-end 等事件上"
            "附着空的 {venice:{}} 命名空间对象；ref 仅在存在实际字段"
            "（messageThoughtSignature/thoughtSignature/encryptedReasoningContent）时附着",
            "ref packages/ai/src/protocols/venice-chat.ts:325-345（条件附着）",
            "rust crates/oc-ai/src/protocols/openai_chat.rs:72,1477（venice 状态机无条件附着空命名空间）",
        ),
    ),
    (
        "venice encrypted reasoning text passthrough",
        lambda scope, d: "__ENCRYPTED_REASONING__" in d,
        (
            "Venice 把加密推理以 `__ENCRYPTED_REASONING__` 文本内联在 reasoning delta 中流回；"
            "ref 原样保留该 marker 文本（后续靠 venice-chat.ts 的 assistant 回放约定消费），"
            "rust 侧把 marker 段从 reasoning 文本中剥除——两侧对 marker 文本的流内语义不同",
            "ref packages/ai/src/protocols/venice-chat.ts:180（MARKER）",
            "rust crates/oc-ai/src/protocols/openai_chat.rs:72,1477（VENICE_MARKER 剥除逻辑）",
        ),
    ),
    (
        "deepinfra estimated_cost float precision",
        lambda scope, d: "estimated_cost" in d,
        (
            "同一 wire 值 2.8780000000000002e-05：ref 原样保留 f64 wire 数值，rust 的 "
            "map_usage→providerMetadata 通路把数值精度截断（2.878e-05，疑似 f32 中转），"
            "usage.providerMetadata 数值不再逐位等于 wire",
            "ref packages/ai/src/protocols/openai-chat.ts（usage passthrough）",
            "rust crates/oc-ai/src/protocols/openai_chat.rs:432-457（map_usage/json_value_map）",
        ),
    ),
]


def classify(name: str, diff: str) -> str | None:
    scope = name
    for label, match, _ in CAUSES:
        if match(scope, diff):
            return label
    return None


def write_report(path: Path, cases: list[dict], census: dict, ref_recs, rust_recs, divergences: list[dict]) -> None:
    ref_by = {r["name"]: r for r in ref_recs}
    rust_by = {r["name"]: r for r in rust_recs}
    fam_counts: dict[str, dict] = {}
    for case in cases:
        fam = fam_counts.setdefault(case["family"], {"cases": 0, "match": 0, "diverge": 0})
        fam["cases"] += 1
        if any(d["name"] == case["name"] for d in divergences):
            fam["diverge"] += 1
        else:
            fam["match"] += 1
    out_groups: dict[str, dict] = {g: s for g, s in sorted(census.items()) if s["out"] and not s["in"]}
    part_groups = {g: s for g, s in sorted(census.items()) if s["out"] and s["in"]}
    lines = []
    lines.append("# PROVIDER-RECORDINGS — provider 协议解码差分（通道 7）")
    lines.append("")
    lines.append("同一录制的响应体（SSE 文本 / JSON body，ref HttpRecorder cassette 真实 provider 流量）")
    lines.append("喂双侧协议解码器，diff 归一化后的 LLMEvent 解码事件流。请求体不参与解码，仅提取")
    lines.append("provider/model id 作为解码器 initial 状态的 harness 上下文。")
    lines.append("")
    lines.append("- rust 侧: `crates/oc-ai/tests/provider_recordings_diff_driver.rs`（env 门控")
    lines.append("  `PROVIDER_DIFF_MANIFEST/OUT`）— 生产 route + `MockTransport` 逐响应体回放（与")
    lines.append("  cassette parity 测试同一入口）。驱动面: crates/oc-ai/tests/provider_recordings_diff_driver.rs:1")
    lines.append("- ref 侧: `tools/diff/drivers/provider_recordings_ref.ts`（bun staging，node_modules →")
    lines.append("  ref/packages/ai/node_modules）— 直接驱动 packages/ai/src/protocols/* 的真实")
    lines.append("  step 机（stream.event 解码 + initial + step + terminal/onHalt + 路由级 SSE framing）。")
    lines.append("  驱动面: tools/diff/drivers/provider_recordings_ref.ts:1")
    lines.append("")
    lines.append("归一化声明: 键序递归排序；text-*/reasoning-* 事件的本地 part id 剥离（生成源差异，")
    lines.append("双侧一致剥离；tool-call id 为 wire 数据保留）；finish reason raw/normalized 小写归一；")
    lines.append("数值按值比较（0==0.0）；usage/providerMetadata 全量保留比较；解码错误只比较 ok 位与")
    lines.append("错误前事件前缀（错误文案不比较）——双侧都错=MATCH，单侧错=DIVERGE。")
    lines.append("")
    total_match = sum(f["match"] for f in fam_counts.values())
    total_div = sum(f["diverge"] for f in fam_counts.values())
    lines.append(f"## 总量: {len(cases)} 个可驱动交互，MATCH {total_match}，DIVERGE {total_div}")
    lines.append("")
    lines.append("| 协议族 | 交互数 | MATCH | DIVERGE |")
    lines.append("|---|---|---|---|")
    for fam in sorted(fam_counts):
        s = fam_counts[fam]
        lines.append(f"| {fam} | {s['cases']} | {s['match']} | {s['diverge']} |")
    lines.append("")
    out_total = sum(s["out"] for s in census.values())
    cassettes_total = sum(s["cassettes"] for s in census.values())
    lines.append(f"## 范围外: {out_total} 个交互（cassette 目录 {len(out_groups)} 组整组范围外 + 混合 cassette 内未映射端点）")
    lines.append("")
    lines.append("整组范围外（原因）:")
    lines.append("")
    lines.append("| 组 | cassette 数 | 范围外交互 | 原因 |")
    lines.append("|---|---|---|---|")
    for g, s in out_groups.items():
        lines.append(f"| {g} | {s['cassettes']} | {s['out']} | {s['reason'] or OUT_GROUPS.get(g, '')} |")
    lines.append("")
    if part_groups:
        lines.append("混合 cassette 部分范围外:")
        lines.append("")
        lines.append("| 组 | cassette 数 | 纳入 | 范围外 | 原因 |")
        lines.append("|---|---|---|---|---|")
        for g, s in part_groups.items():
            lines.append(f"| {g} | {s['cassettes']} | {s['in']} | {s['out']} | {s['reason']} |")
        lines.append("")
    lines.append(f"cassette 总数（目录内 .json）: {cassettes_total}；语料含 4 个顶层散装 fixture（evaluation，非 cassette 格式，范围外）。")
    lines.append("")
    lines.append("## 分歧明细")
    lines.append("")
    if not divergences:
        lines.append("无分歧。")
    else:
        for d in divergences:
            lines.append(f"### {d['name']} — {d['family']}")
            lines.append(f"- 差异: `{d['diff']}`")
            r_rust, r_ref = rust_by.get(d["name"], {}), ref_by.get(d["name"], {})
            lines.append(f"- rust ok={r_rust.get('ok')} events={len(r_rust.get('events', []))} error={bool(r_rust.get('error'))}")
            lines.append(f"- ref  ok={r_ref.get('ok')} events={len(r_ref.get('events', []))} error={bool(r_ref.get('error'))}")
            lines.append("")
    lines.append("## 定性")
    lines.append("")
    if divergences:
        by_label: dict[str, list[str]] = {}
        for d in divergences:
            label = classify(d["name"], d["diff"]) or "未裁决候选"
            by_label.setdefault(label, []).append(d)
        for label, ds in by_label.items():
            lines.append(f"### {label}（{len(ds)} 个交互）")
            cause = next((c for l, _, c in CAUSES if l == label), None)
            if cause:
                lines.append(f"- 判定: {cause[0]}")
                lines.append(f"- ref : {cause[1]}")
                lines.append(f"- rust: {cause[2]}")
            lines.append(f"- 交互: {', '.join(d['name'] for d in ds)}")
            lines.append("")
    else:
        lines.append("- 全量一致。")
    lines.append("")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines))
    print(f"wrote {path}")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ref", default=str(REF_DEFAULT))
    ap.add_argument("--write", default="docs/audit/diff/PROVIDER-RECORDINGS.md")
    ap.add_argument("--manifest-only", action="store_true")
    args = ap.parse_args()
    ref = Path(args.ref)

    manifest = STAGE / "manifest.json"
    cases, census = build_manifest(ref, STAGE)
    manifest.write_text(json.dumps(cases, indent=1))
    sse = sum(1 for c in cases if "event-stream" in c["content_type"])
    print(f"manifest: {len(cases)} interactions ({sse} SSE bodies) -> {manifest}")
    if args.manifest_only:
        return 0

    ref_out = STAGE / "ref-results.json"
    rust_out = STAGE / "rust-results.json"
    run_ref(STAGE, ref, manifest, ref_out)
    run_rust(manifest, rust_out)

    ref_recs = json.loads(ref_out.read_text())
    rust_recs = json.loads(rust_out.read_text())
    ref_by = {r["name"]: norm_case(r) for r in ref_recs}
    rust_by = {r["name"]: norm_case(r) for r in rust_recs}
    divergences = []
    for case in cases:
        name = case["name"]
        a, b = rust_by.get(name), ref_by.get(name)
        if a is None or b is None:
            divergences.append({"name": name, "family": case["family"], "diff": "missing driver result"})
            continue
        d = first_diff(a, b)
        if d:
            divergences.append({"name": name, "family": case["family"], "diff": d})
    write_report(Path(args.write), cases, census, ref_recs, rust_recs, divergences)
    print(f"verdicts: {len(cases) - len(divergences)} MATCH, {len(divergences)} DIVERGE")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
