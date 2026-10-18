#!/usr/bin/env python3
"""B0-1 parity ledger importer.

Deterministically imports the 16 detail reports in
docs/parity-scan-2026-10-18/ into docs/parity-ledger.jsonl.

Design (docs/PARITY-LOOP.md §3.6):
  * parse each report's difference records (format-aware per report);
  * fail loudly (exit 1) if the parsed record set does not match the
    hard-coded MANIFEST — no silent drops;
  * keep the 114 canonical seed rows byte-stable; they may only gain
    `aliases` when a report record folds into them;
  * every remaining record becomes a new canonical row (fields per §3.2)
    or is folded as an alias into another canonical row (same root cause);
  * zero-orphan guarantee: every enumerated raw report ID ends up as some
    row's source_id or alias;
  * stdlib only, no wall clock, no randomness, fixed date 2026-10-18,
    stable sort everywhere — re-runs are byte-identical.

Usage:
  python3 tools/parity-import.py            # write ledger + print stats
  python3 tools/parity-import.py --check    # parse + verify only, no write
"""
from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCAN = ROOT / "docs" / "parity-scan-2026-10-18"
LEDGER = ROOT / "docs" / "parity-ledger.jsonl"
DATE = "2026-10-18"  # fixed; determinism

REPORT_FILES = {
    1: "01-session.md",
    2: "02-tools.md",
    3: "03-ai.md",
    4: "04-provider-auth.md",
    5: "05-api.md",
    6: "06-tui.md",
    7: "07-cli-config.md",
    8: "08-plugin-platform.md",
    9: "09-database.md",
    10: "10-surfaces.md",
    11: "11-core-misc.md",
    12: "12-http-payloads.md",
    13: "13-config-env-policy.md",
    14: "14-tui-deep.md",
    15: "15-event-payloads.md",
    16: "16-copilot-media.md",
}

# MASTER 00-MASTER.md §0.2 per-report counts (for the reconciliation delta).
MASTER_COUNTS = {
    1: 34, 2: 33, 3: 26, 4: 22, 5: 22, 6: 34, 7: 20, 8: 20, 9: 20,
    10: 21, 11: 26, "12-16": 108,
}

CLUSTER_DOMAIN = {
    "CRED": "provider-auth",
    "PLUGIN": "plugin",
    "RUNTIME": "session",
    "WIRE": "http",
    "CONFIG": "config",
    "TUI": "tui",
    "AI": "ai",
    "DB": "database",
    "SURFACES": "surface",
    "TOOLS-EXEC": "tools",
}

CLUSTER_TESTFILE = {
    "CRED": "parity_credentials.rs",
    "PLUGIN": "parity_plugin_platform.rs",
    "RUNTIME": "parity_session_lifecycle.rs",
    "WIRE": "parity_http_contract.rs",
    "CONFIG": "parity_config_env.rs",
    "TUI": "parity_tui_behavior.rs",
    "AI": "parity_ai_providers.rs",
    "DB": "parity_database.rs",
    "SURFACES": "parity_surfaces.rs",
    "TOOLS-EXEC": "parity_tools_exec.rs",
}

CLUSTER_CRATE = {
    "CRED": "oc-core",
    "PLUGIN": "oc-plugin-host",
    "RUNTIME": "oc-core",
    "WIRE": "oc-server",
    "CONFIG": "oc-core",
    "TUI": "oc-tui",
    "AI": "oc-ai",
    "DB": "oc-core",
    "SURFACES": "oc-cli",
    "TOOLS-EXEC": "oc-core",
}
# secondary crate preferences when a cited file exists in several crates
CLUSTER_CRATE_PREF = {
    "RUNTIME": ["oc-core", "oc-server"],
    "TOOLS-EXEC": ["oc-core", "oc-server"],
    "AI": ["oc-ai", "oc-core"],
    "CONFIG": ["oc-core", "oc-cli", "oc-tui"],
    "WIRE": ["oc-server", "oc-core"],
    "CRED": ["oc-core", "oc-server"],
    "DB": ["oc-core", "oc-server"],
    "TUI": ["oc-tui"],
    "PLUGIN": ["oc-plugin-host", "oc-core"],
    "SURFACES": ["oc-cli", "oc-server"],
}

UNVERIFIED_MARKERS = (
    "UNVERIFIED", "静态推断", "未证实", "待运行时复现", "需运行时复现", "需实测", "未验证",
)

STOPWORDS = {
    "the", "and", "for", "not", "with", "ref", "rust", "v2", "vs", "NEW",
    "KNOWN", "MISSING", "WRONG", "PARTIAL", "STUB", "EXTRA", "TODO",
}


class Rec:
    """One raw difference record parsed from a report."""

    def __init__(self, report: int, rid: str, **kw):
        self.report = report
        self.rid = rid
        self.title = kw.get("title", "")
        self.sev = kw.get("sev", "S2")
        self.typ = kw.get("typ", "")
        self.known = kw.get("known", "")
        self.expected = kw.get("expected", "")
        self.actual = kw.get("actual", "")
        self.impact = kw.get("impact", "")
        self.refs = kw.get("refs", [])
        self.rusts = kw.get("rusts", [])
        self.body = kw.get("body", "")

    @property
    def key(self) -> str:
        return f"{self.report:02d}:{self.rid}"

    def text(self) -> str:
        return " ".join([self.title, self.body, self.expected, self.actual, self.impact])


# ---------------------------------------------------------------------------
# generic helpers
# ---------------------------------------------------------------------------

def cells(line: str) -> list[str]:
    parts = re.split(r"(?<!\\)\|", line.strip().strip("|"))
    return [c.strip().replace("\\|", "|") for c in parts]


def is_table_row(line: str) -> bool:
    s = line.strip()
    return s.startswith("|") and s.endswith("|") and s.count("|") >= 3


PATH_RE = re.compile(
    r"(?<![\w./-])((?:[\w.-]+/)*[\w.-]+\.(?:rs|ts|tsx|js|json|toml|yml|yaml))"
    r"(?::\d+(?:-\d+)?(?::\d+(?:-\d+)?)*)?"
)


def _is_rust_path(p: str) -> bool:
    file = p.split(":", 1)[0]
    return file.endswith((".rs", ".toml")) or file.startswith("crates/") or re.match(r"^oc-[\w-]+/", file)


def split_paths(text: str) -> tuple[list[str], list[str]]:
    """Split `path[:line]` mentions found in *text* into ref / rust lists."""
    refs, rusts = [], []
    for m in PATH_RE.finditer(text or ""):
        p = m.group(0)
        if p.startswith("http"):
            continue
        if _is_rust_path(p):
            if p not in rusts:
                rusts.append(p)
        else:
            if p not in refs:
                refs.append(p)
    return refs, rusts


def norm_path_item(item: str) -> str:
    """Reduce a messy table cell to its leading `path[:line]` (or cleaned text)."""
    item = re.sub(r"[*`]", "", item or "").strip()
    m = PATH_RE.search(item)
    if m and m.start() <= 4:
        return m.group(0)
    return item


def norm_sev(s: str) -> str:
    s = re.sub(r"[*`]", "", s or "").strip()
    m = re.match(r"(S[0-3])", s)
    return m.group(1) if m else "S2"


def clean(s: str, limit: int = 240) -> str:
    s = re.sub(r"[*`]", "", s or "").strip()
    s = re.sub(r"\s+", " ", s)
    return s[:limit]


def slug_code(title: str, fallback: str) -> str:
    words: list[str] = []
    for tok in re.findall(r"[A-Za-z][A-Za-z0-9_.#/=-]+", title or ""):
        for w in re.split(r"[^A-Za-z0-9]+", tok.replace("/", " ").replace("=", " ").replace("#", " ")):
            w = w.strip("._-")
            if len(w) < 2 or w.lower() in STOPWORDS or w.upper() in STOPWORDS \
                    or w.lower() in ("ts", "tsx", "rs", "js", "json", "tsx"):
                continue
            if w.lower() not in [x.lower() for x in words]:
                words.append(w)
    if not words:
        return fallback.lower().replace("_", "-")
    code = "-".join(w.lower()[:14] for w in words[:4])
    return code[:48].rstrip("-") or fallback.lower().replace("_", "-")


def section(text: str, start_pat: str, end_pat: str | None = None) -> str:
    lines = text.splitlines()
    out, inside = [], False
    for ln in lines:
        if not inside and re.match(start_pat, ln):
            inside = True
            continue
        if inside and end_pat and re.match(end_pat, ln):
            break
        if inside:
            out.append(ln)
    return "\n".join(out)


# ---------------------------------------------------------------------------
# per-report parsers — each returns list[Rec] and must match MANIFEST exactly
# ---------------------------------------------------------------------------

def parse_01(text: str) -> list[Rec]:
    """Tables `| ID | 标题 | 类型 | 严重度 | ref | rust | 差异 | 用户可见影响 |`."""
    recs = []
    for ln in text.splitlines():
        if not is_table_row(ln):
            continue
        c = cells(ln)
        if len(c) < 8 or not re.match(r"^[A-Z]{1,2}-\d{2}$", c[0]):
            continue
        diff = clean(c[6], 400)
        em = re.search(r"EXPECTED[^\w]*?(.*?)(?:ACTUAL|$)", diff)
        am = re.search(r"ACTUAL[^\w]*?(.*)$", diff)
        refs, rusts = split_paths(c[4] + " " + c[5])
        recs.append(Rec(1, c[0], title=clean(c[1], 100), sev=norm_sev(c[3]),
                        typ=clean(c[2], 30), expected=(em.group(1) if em else clean(refs and refs[0] or "", 300)),
                        actual=(am.group(1) if am else clean(rusts and rusts[0] or "", 300)),
                        impact=clean(c[7], 300), refs=refs, rusts=rusts,
                        known="KNOWN" if "KNOWN" in c[7] else "NEW"))
    return recs


def parse_02(text: str) -> list[Rec]:
    """§1.2 diff table `| ID | 标题 | 类型 | 严重度 | KNOWN/NEW |` + **ID** detail bodies."""
    meta = {}
    for ln in text.splitlines():
        if not is_table_row(ln):
            continue
        c = cells(ln)
        if len(c) >= 5 and re.match(r"^[A-Z]\d+$", c[0]):
            meta[c[0]] = (clean(c[1], 100), clean(c[2], 30), norm_sev(c[3]), clean(c[4], 60))
    bodies: dict[str, dict] = {}
    cur = None
    for ln in text.splitlines():
        m = re.match(r"^\*\*([A-Z]\d+)\*\*\s*(.*)$", ln.strip())
        if m:
            cur = m.group(1)
            bodies[cur] = {"title": clean(m.group(2), 100), "ref": "", "rust": "", "impact": "", "extra": []}
            continue
        if cur:
            s = ln.strip()
            m2 = re.match(r"^- ref[：:]\s*(.*)$", s)
            m3 = re.match(r"^- rust[：:]\s*(.*)$", s)
            if m2:
                bodies[cur]["ref"] = clean(m2.group(1), 400)
            elif m3:
                bodies[cur]["rust"] = clean(m3.group(1), 400)
            elif s.startswith("- 影响") or s.startswith("-影响"):
                bodies[cur]["impact"] = clean(s.split("：", 1)[-1], 300)
            elif re.match(r"^## ", ln):
                cur = None
    recs = []
    for rid in sorted(set(meta) | set(bodies)):
        t, typ, sev, kn = meta.get(rid, ("", "", "S2", ""))
        b = bodies.get(rid, {})
        refs, rusts = split_paths(b.get("ref", ""))
        r2, s2 = split_paths(b.get("rust", ""))
        title = t or b.get("title", "")
        recs.append(Rec(2, rid, title=title or rid, sev=sev, typ=typ,
                        expected=b.get("ref", ""), actual=b.get("rust", ""),
                        impact=b.get("impact", ""), refs=refs + r2, rusts=rusts + s2,
                        known=kn))
    return recs


def parse_03(text: str) -> list[Rec]:
    """M-table (§2) + protocol tables (§3.1/3.3/3.4/3.5) + curated section records."""
    recs = []
    # §2 M table: | # | 项 | ref 证据 | 严重度 | KNOWN/NEW |
    sec = section(text, r"^## 2\. ", r"^## 3\. ")
    for ln in sec.splitlines():
        if not is_table_row(ln):
            continue
        c = cells(ln)
        if len(c) >= 5 and re.match(r"^M\d+$", c[0]):
            refs, _ = split_paths(c[2])
            recs.append(Rec(3, c[0], title=clean(c[1], 110), sev=norm_sev(c[3]),
                            refs=refs, known=clean(c[4], 80),
                            expected=clean(c[2], 350), actual="见影响（协议未接线/缺失）"))
    # §3 protocol tables: | # | 差异 | EXPECTED | ACTUAL | 影响 | 级 |
    for start, end in ((r"^### 3\.1 ", r"^### 3\.2 "), (r"^### 3\.3 ", r"^### 3\.4 "),
                       (r"^### 3\.4 ", r"^### 3\.5 "), (r"^### 3\.5 ", r"^### 3\.6 ")):
        sec = section(text, start, end)
        for ln in sec.splitlines():
            if not is_table_row(ln):
                continue
            c = cells(ln)
            if len(c) >= 6 and re.match(r"^[ARVB]\d*$", c[0]) and norm_sev(c[5]) in ("S0", "S1", "S2", "S3"):
                if c[5].strip().startswith("—") or c[5].strip() == "":
                    continue
                refs, rusts = split_paths(c[2] + " " + c[3])
                recs.append(Rec(3, c[0], title=clean(c[1], 110), sev=norm_sev(c[5]),
                                expected=clean(c[2], 350), actual=clean(c[3], 350),
                                impact=clean(c[4], 250), refs=refs, rusts=rusts))
    # curated section records (bodies parsed for EXPECTED/ACTUAL)
    curated = [
        ("§3.6", r"^### 3\.6 ", r"^### 3\.7 ", "Cohere/Mistral 原生协议未接线（运行时错线）"),
        ("§3.7", r"^### 3\.7 ", r"^### 3\.8 ", "GitHub Copilot responses lane 缺失（03 首登）"),
        ("§3.9", r"^### 3\.9 ", r"^## 4\. ", "Azure URL/鉴权头/deployments 全缺"),
        ("5.1", r"^### 5\.1 ", r"^### 5\.2 ", "HTTP 超时预算未实现（header/chunk/total）"),
        ("5.2", r"^### 5\.2 ", r"^### 5\.3 ", "abort/取消语义无专门 parity 测试"),
    ]
    for rid, s, e, title in curated:
        body = section(text, s, e)
        em = re.search(r"- EXPECTED[：:](.*?)(?=\n- |\Z)", body, re.S)
        am = re.search(r"- ACTUAL[：:](.*?)(?=\n- |\Z)", body, re.S)
        refs, rusts = split_paths(body)
        sev = "S1" if rid in ("§3.6", "§3.9", "5.1") else "S2"
        recs.append(Rec(3, rid, title=title, sev=sev, typ="MISSING",
                        expected=clean(em.group(1), 400) if em else "",
                        actual=clean(am.group(1), 400) if am else "",
                        body=clean(body, 500), refs=refs, rusts=rusts))
    return recs


def parse_04(text: str) -> list[Rec]:
    """§5 差异索引总表: | ID | 级别 | 状态 | KNOWN/NEW | 摘要 | ref | rust |."""
    recs = []
    sec = section(text, r"^## 5\. 差异索引总表", r"^## 6\. ")
    for ln in sec.splitlines():
        if not is_table_row(ln):
            continue
        c = cells(ln)
        if len(c) >= 7 and c[0] != "ID" and re.match(r"^[A-Z]+[-/]?[\w/]*$", c[0]) and norm_sev(c[1]) in ("S0", "S1", "S2", "S3"):
            refs, rusts = split_paths(c[5] + " " + c[6])
            recs.append(Rec(4, c[0], title=clean(c[4], 110), sev=norm_sev(c[1]),
                            typ=clean(c[2], 30), known=clean(c[3], 90),
                            refs=refs, rusts=rusts,
                            expected="ref: " + clean(c[5], 300) if c[5] else "",
                            actual="rust: " + clean(c[6], 300) if c[6] else ""))
    return recs


def parse_05(text: str) -> list[Rec]:
    """Tables `| # | 分类 | 差异 | ref | rust | 影响/登记 |`; severity from section."""
    recs = []
    sev = "S2"
    for ln in text.splitlines():
        m = re.match(r"^### (S\d)", ln.strip())
        if m:
            sev = m.group(1)
            continue
        if not is_table_row(ln):
            continue
        c = cells(ln)
        if len(c) >= 6 and re.match(r"^F\d+$", c[0]):
            refs, rusts = split_paths(c[3] + " " + c[4])
            if not refs and c[3]:
                refs = [clean(c[3], 120)]
            if not rusts and c[4]:
                rusts = [clean(c[4], 120)]
            recs.append(Rec(5, c[0], title=clean(c[2], 110), sev=sev,
                            typ=clean(c[1], 30), expected=clean(c[3], 350),
                            actual=clean(c[4], 350), impact=clean(c[5], 300),
                            refs=refs, rusts=rusts,
                            known="KNOWN" if "KNOWN" in c[5] else "NEW"))
    return recs


def parse_06(text: str) -> list[Rec]:
    """§14 table: | # | 级/类/登记 | EXPECTED（ref） | ACTUAL（rust） | 影响 |."""
    recs = []
    sec = section(text, r"^## 14\. ", r"^## 15\. ")
    for ln in sec.splitlines():
        if not is_table_row(ln):
            continue
        c = cells(ln)
        if len(c) >= 5 and re.match(r"^\d+$", c[0]):
            grade = clean(c[1], 60)
            sev = norm_sev(grade)
            typ = re.search(r"(MISSING|STUB|PARTIAL|WRONG|EXTRA)", grade)
            refs, rusts = split_paths(c[2] + " " + c[3])
            recs.append(Rec(6, "#" + c[0], title=clean(c[2], 100), sev=sev,
                            typ=typ.group(1) if typ else "",
                            expected=clean(c[2], 350), actual=clean(c[3], 350),
                            impact=clean(c[4], 250), refs=refs, rusts=rusts,
                            known="KNOWN" if "KNOWN" in grade else "NEW",
                            body=grade))
    return recs


def parse_07(text: str) -> list[Rec]:
    """F-table (| F-x | 差异 | ref | rust | 级别 |) + N-table (| N-x | 摘要 | 级别 | K/N |)."""
    recs = []
    for ln in text.splitlines():
        if not is_table_row(ln):
            continue
        c = cells(ln)
        if len(c) >= 4 and re.match(r"^F-\d+$", c[0]):
            if len(c) >= 5:
                ref_txt, rust_txt, sev_txt = c[2], c[3], c[4]
            else:
                parts = c[2].split(" vs ", 1)
                ref_txt = parts[0]
                rust_txt = parts[1] if len(parts) > 1 else ""
                sev_txt = c[3]
            refs = [p.strip() for p in ref_txt.split(" vs ") if p.strip()]
            rusts = [p.strip() for p in rust_txt.split(" vs ") if p.strip()]
            recs.append(Rec(7, c[0], title=clean(c[1], 120), sev=norm_sev(sev_txt),
                            refs=refs, rusts=rusts,
                            expected=clean(ref_txt, 300), actual=clean(rust_txt, 300),
                            known=clean(sev_txt, 60)))
        elif len(c) >= 4 and re.match(r"^N-\d+$", c[0]):
            recs.append(Rec(7, c[0], title=clean(c[1], 120), sev=norm_sev(c[2]),
                            known=clean(c[3], 60)))
    return recs


def parse_08(text: str) -> list[Rec]:
    """§4 bullet records: `- **D-XX ｜ TYPE ｜ SEV ｜ K/N** — title.` + EXPECTED/ACTUAL/影响."""
    recs = []
    cur = None
    for ln in text.splitlines():
        s = ln.strip()
        m = re.match(r"^- \*\*(D-\d+)\s*｜\s*(.+?)\s*｜\s*(S\d+)\s*｜\s*(.*?)\*\*\s*[—-]?\s*(.*)$", s)
        if m:
            cur = Rec(8, m.group(1), title=clean(m.group(5) or m.group(2), 110),
                      sev=m.group(3), typ=clean(m.group(2), 30), known=clean(m.group(4), 120),
                      expected="", actual="", impact="", body="")
            recs.append(cur)
            continue
        if cur is not None:
            if re.match(r"^## ", ln):
                cur = None
                continue
            em = re.match(r"^\s*EXPECTED[：:](.*)$", s)
            am = re.match(r"^\s*ACTUAL[：:](.*)$", s)
            im = re.match(r"^\s*影响[：:](.*)$", s)
            if em:
                cur.expected = clean(em.group(1), 400)
            elif am:
                cur.actual = clean(am.group(1), 400)
            elif im:
                cur.impact = clean(im.group(1), 300)
            cur.body += s + "\n"
    for r in recs:
        refs, rusts = split_paths(r.body)
        r.refs, r.rusts = refs, rusts
    return recs


def parse_09(text: str) -> list[Rec]:
    """§13 table: | ID | 判定 | 级别 | K/N | 摘要 | ref | rust |."""
    recs = []
    sec = section(text, r"^## 13\. ", r"^## 14\. ")
    for ln in sec.splitlines():
        if not is_table_row(ln):
            continue
        c = cells(ln)
        if len(c) >= 7 and re.match(r"^D\d+$", c[0]):
            refs, rusts = split_paths(c[5] + " " + c[6])
            sev = norm_sev(c[2]) if not c[2].startswith("—") else "S2"
            recs.append(Rec(9, c[0], title=clean(c[4], 110), sev=sev,
                            typ=clean(c[1], 30), known=clean(c[3], 60),
                            refs=refs, rusts=rusts,
                            expected="ref: " + clean(c[5], 300),
                            actual="rust: " + clean(c[6], 300)))
    return recs


def parse_10(_: str) -> list[Rec]:
    """Report 10 has no per-record ID table; curated records anchored in §2/§4."""
    curated = [
        ("P0-browser", "browser 工具（plugin-browser）缺失", "S1", "MISSING", "NEW",
         "packages/core/src/plugin/internal.ts:89；packages/plugin-browser/src/tools.ts:36",
         "v2 会话内 agent 可浏览网页/操作页面；Rust oc-core/src/tool/ 无此工具"),
        ("P0-latex", "TUI 内建 LaTeX / mermaid 渲染缺失", "S1", "MISSING", "NEW",
         "packages/tui/src/plugin/builtins.ts:24-26（Stats, Merman, Latex 默认注册）",
         "Rust 全仓 grep latex|mermaid|katex 零命中，回退纯文本"),
        ("P1-sdk", "npm 包 @opencode-ai/sdk（JS SDK）缺失", "S2", "MISSING", "NEW",
         "packages/sdk/src/opencode.ts 等",
         "JS/TS 生态编程接入入口缺失；Rust 仅自用 oc-client"),
        ("P1-vscode", "VS Code 扩展缺失", "S2", "MISSING", "NEW",
         "sdks/vscode/src/extension.ts:6-26；workflows/publish-vscode.yml（ref 仓）",
         "编辑器集成面完全缺失"),
        ("P1-gha", "GitHub Action（/opencode 评论式 CI）缺失", "S2", "MISSING", "NEW",
         "github/action.yml；workflows/publish-github-action.yml（ref 仓）",
         "评论式 CI 自动化缺失"),
        ("P1-storybook", "TUI storybook feature-plugin 缺失", "S2", "MISSING", "NEW",
         "builtins.ts:29（OPENCODE_STORY 门控）",
         "仅开发可见（严格说 P2，报告列 P1 备查）"),
        ("P2-containers", "packages/containers（发布镜像/AUR 工具链）缺失", "S2", "MISSING", "NEW",
         "packages/containers", "开发/发布工具缺口"),
        ("P2-simulation", "packages/simulation 缺失", "S2", "MISSING", "NEW",
         "packages/simulation（routes.ts:151-156 注入缝）", "开发工具缺口"),
        ("P2-storybook", "packages/storybook 缺失", "S2", "MISSING", "NEW",
         "packages/storybook", "开发工具缺口"),
        ("P2-codegen", "packages/httpapi-codegen（client 生成器）半缺", "S2", "MISSING", "NEW",
         "packages/httpapi-codegen", "生成器能力半缺（tools/openapi-gen 为对应物）"),
        ("P2-recorder", "packages/http-recorder（录制端）半缺", "S2", "MISSING", "NEW",
         "packages/http-recorder", "cassette 录制端半缺"),
        ("P2-script", "packages/script 缺失", "S2", "MISSING", "NEW",
         "packages/script", "脚本工具缺口"),
        ("P2-drizzle", "packages/effect-drizzle-sqlite 缺失", "S2", "MISSING", "NEW",
         "packages/effect-drizzle-sqlite", "Effect 桥接库（Rust 侧由 rusqlite 替代生态位）"),
        ("P2-identity", "packages/identity（资产）缺失", "S2", "MISSING", "NEW",
         "packages/identity", "品牌资产包缺口"),
        ("P3-console", "packages/console/*（6 子包，托管 Console 站）缺失", "S3", "MISSING", "KNOWN",
         "packages/console/console.ts 等", "托管运营面（连接侧翼 oc-core/integration/console.rs 已有）"),
        ("P3-enterprise", "packages/enterprise 缺失", "S3", "MISSING", "KNOWN",
         "packages/enterprise", "托管部署面（配置面 oc-schema config/mod.rs:73-75 已覆盖）"),
        ("P3-function", "packages/function 缺失", "S3", "MISSING", "KNOWN",
         "packages/function", "托管功能面"),
        ("P3-stats", "packages/stats/*（托管统计站）缺失", "S3", "MISSING", "KNOWN",
         "packages/stats/*", "CLI stats 命令已移植（oc-cli/handlers/stats.rs），托管站缺"),
        ("P3-posts", "packages/posts 缺失", "S3", "MISSING", "KNOWN",
         "packages/posts", "博客/内容面"),
        ("P3-services", "services/{www,update,files} 缺失", "S3", "MISSING", "KNOWN",
         "services/{www,update,files}", "托管服务面（升级通道已换道 GitHub Releases）"),
        ("PLATFORM", "平台/分发矩阵：仅 linux-x64 一个 build job", "S1", "MISSING", "KNOWN",
         "publish.yml:44-198（linux/windows/macos-26 签名）；AUR/npm 渠道",
         "release.yml:85 仅 linux-x64；npm、AUR、Homebrew 渠道全缺（ROADMAP R-11）"),
    ]
    recs = []
    for rid, title, sev, typ, kn, ref, impact in curated:
        refs, rusts = split_paths(ref)
        recs.append(Rec(10, rid, title=title, sev=sev, typ=typ, known=kn,
                        expected=clean(ref, 300), actual="无对应（见 10-surfaces.md §2/§4）",
                        impact=clean(impact, 300), refs=refs, rusts=rusts))
    return recs


def parse_11(text: str) -> list[Rec]:
    """§16 recap tables (#1-#15) + curated §1 map rows (map-N)."""
    recs = []
    s1 = section(text, r"^### S1（线格式", r"^### S2（行为分歧")
    s2 = section(text, r"^### S2（行为分歧", r"^### 无发现")
    for ln in s1.splitlines():
        if not is_table_row(ln):
            continue
        c = cells(ln)
        if len(c) >= 5 and re.match(r"^\d+$", c[0]):
            refs, rusts = split_paths(c[2] + " " + c[3])
            recs.append(Rec(11, "#" + c[0], title=clean(c[1], 130), sev="S1",
                            refs=refs, rusts=rusts, expected=clean(c[2], 350),
                            actual=clean(c[3], 350)))
    for ln in s2.splitlines():
        if not is_table_row(ln):
            continue
        c = cells(ln)
        if len(c) >= 4 and re.match(r"^\d+$", c[0]):
            refs, rusts = split_paths(c[2])
            recs.append(Rec(11, "#" + c[0], title=clean(c[1], 130), sev="S2",
                            refs=refs, rusts=rusts, expected=clean(c[2], 350),
                            actual=clean(c[3], 80), typ=clean(c[3], 40)))
    # curated §1 map rows (record-identity rows not subsumed by the recap)
    wanted = {
        "1": "codemode catalog summarize 预算算法 PARTIAL",
        "5": "environment/environment.ts Files+spawner 缝（架构性）",
        "6": "environment/local.ts 内联本地 fs",
        "7": "environment/exec-defaults.ts（sh 脚本+64MiB 上限）",
        "11": "modal/models.ts 动态模型 load/derive",
        "13": "git.ts repo.clone",
        "14": "git.ts repo.create（snapshot 仓库 init + opencode.gitconfig seed）",
        "16": "git.ts history.branch（symbolic-ref）/defaultRemoteBranch",
        "17": "git.ts sync.*（fetchRemotes/fetchBranch/checkoutRemoteBranch/resetHard）",
        "21": "git.ts index.refresh / ignored（scope 级索引刷新）",
        "22": "git.ts tree.capture/write/files/diff/restore",
    }
    sec = section(text, r"^## 1\. 参考模块", r"^## 2\. ")
    for ln in sec.splitlines():
        if not is_table_row(ln):
            continue
        c = cells(ln)
        if len(c) >= 7 and c[0] in wanted:
            refs, rusts = split_paths(c[1] + " " + c[2])
            sev = "S1" if "S1" in c[5] and "S1/S2" not in c[5] else "S2"
            recs.append(Rec(11, "map-" + c[0], title=clean(c[1], 130), sev=sev,
                            typ=clean(c[3], 30), known=clean(c[6], 80),
                            refs=refs, rusts=rusts,
                            expected="ref: " + clean(c[1], 300),
                            actual="rust: " + clean(c[2], 300)))
    return recs


def parse_12(text: str) -> list[Rec]:
    """Four tables `| ID | endpoint | 字段 | EXPECTED | ACTUAL | 严重度 | NEW |` + H0."""
    recs = []
    for ln in text.splitlines():
        if not is_table_row(ln):
            continue
        c = cells(ln)
        if len(c) >= 7 and re.match(r"^H\d+$", c[0]):
            refs, rusts = split_paths(c[3] + " " + c[4])
            recs.append(Rec(12, c[0], title=clean(c[1] + " " + c[2], 120),
                            sev=norm_sev(c[5]), expected=clean(c[3], 400),
                            actual=clean(c[4], 400), refs=refs, rusts=rusts,
                            known=clean(c[6], 80)))
    return recs


def parse_13(text: str) -> list[Rec]:
    """§8 table: | ID | 判定 | 级别 | K/N | 摘要 | ref | rust | (skip '—' cross-refs)."""
    recs = []
    sec = section(text, r"^## 8\. 差异清单汇总", r"^## 9\. ")
    for ln in sec.splitlines():
        if not is_table_row(ln):
            continue
        c = cells(ln)
        if len(c) >= 7 and re.match(r"^[CTE]-\d+$", c[0]):
            sev = "S1" if "S1" in c[2] else "S2"
            refs, rusts = split_paths(c[5] + " " + c[6])
            recs.append(Rec(13, c[0], title=clean(c[4], 130), sev=sev,
                            typ=clean(c[1], 30), known=clean(c[3], 60),
                            refs=refs, rusts=rusts,
                            expected="ref: " + clean(c[5], 300),
                            actual="rust: " + clean(c[6], 300)))
    return recs


def parse_14(text: str) -> list[Rec]:
    """Headers `### TD-XX · SEV · TYPE · K/N [· UNVERIFIED] [—— title]` + bullet bodies."""
    recs = []
    cur = None
    rest_of_header: dict[str, str] = {}
    for ln in text.splitlines():
        s = ln.strip()
        m = re.match(r"^### (TD-\d+)\s*·\s*(.+)$", s)
        if m:
            rid, rest = m.group(1), m.group(2)
            parts = [p.strip() for p in rest.split("·")]
            sev = parts[0] if re.match(r"^(S\d|INFO)$", parts[0]) else "S2"
            title = ""
            for p in parts:
                if "——" in p:
                    title = p.split("——", 1)[1].strip()
                    break
            if not title and "——" in rest:
                title = rest.split("——", 1)[1].strip()
            typ = ""
            for p in parts[1:]:
                if re.match(r"^(MISSING|STUB|PARTIAL|WRONG|EXTRA|无独立差异)", p):
                    typ = p
                    break
            known = " ".join(parts[2:]) if len(parts) > 2 else "NEW"
            rest_of_header[rid] = rest
            cur = Rec(14, rid, title=clean(title, 130) if title else "",
                      sev="S3" if sev == "INFO" else sev, typ=clean(typ, 40),
                      known=clean(known, 90), body=s)
            recs.append(cur)
            continue
        if cur is not None:
            cur.body += s + "\n"
    for r in recs:
        em = re.search(r"\*\*EXPECTED\*\*[：:](.*?)(?=- \*\*|\Z)", r.body, re.S)
        am = re.search(r"\*\*ACTUAL\*\*[：:](.*?)(?=- \*\*|\Z)", r.body, re.S)
        im = re.search(r"\*\*用户可见影响\*\*[：:](.*)", r.body)
        r.expected = clean(em.group(1), 400) if em else ""
        r.actual = clean(am.group(1), 400) if am else ""
        r.impact = clean(im.group(1), 300) if im else ""
        if not r.title:
            src = r.expected or clean(rest_of_header.get(r.rid, ""), 130) or r.rid
            r.title = clean(src, 90)
        r.refs, r.rusts = split_paths(r.body)
    return recs


def parse_15(text: str) -> list[Rec]:
    """§3.1 table: | ID | 事件/字段 | EXPECTED（ref） | ACTUAL（rust） | 严重度 | 状态 |."""
    recs = []
    sec = section(text, r"^### 3\.1 ", r"^### 3\.2 ")
    for ln in sec.splitlines():
        if not is_table_row(ln):
            continue
        c = cells(ln)
        if len(c) >= 6 and re.match(r"^EV-\d+$", c[0].strip("*")):
            sev = norm_sev(c[4]) if not c[4].startswith("—") else "S3"
            refs, rusts = split_paths(c[2] + " " + c[3])
            recs.append(Rec(15, c[0].strip("*"), title=clean(c[1], 120), sev=sev,
                            expected=clean(c[2], 400), actual=clean(c[3], 400),
                            refs=refs, rusts=rusts, known=clean(c[5], 90)))
    return recs


def parse_16(text: str) -> list[Rec]:
    """Headers `#### C1【SEV · K/N】title` + bullet bodies."""
    recs = []
    cur = None
    for ln in text.splitlines():
        s = ln.strip()
        m = re.match(r"^#### (C\d)【(S\d)[^】]*】(.*)$", s)
        if m:
            cur = Rec(16, m.group(1), title=clean(m.group(3), 130), sev=m.group(2),
                      body=s, typ="", known="")
            recs.append(cur)
            continue
        if cur is not None:
            cur.body += s + "\n"
    for r in recs:
        em = re.search(r"EXPECTED[^\n]*?[：:](.*?)(?=\n- |\Z)", r.body, re.S)
        am = re.search(r"ACTUAL[^\n]*?[：:](.*?)(?=\n- |\Z)", r.body, re.S)
        r.expected = clean(em.group(1), 400) if em else ""
        r.actual = clean(am.group(1), 400) if am else ""
        r.refs, r.rusts = split_paths(r.body)
        kn = re.search(r"【S\d+\s*·\s*([NEWKNOWN（()：: /\d§#.a-z-]+)】", r.body)
        r.known = clean(kn.group(1), 90) if kn else "NEW"
    return recs


PARSERS = {
    1: parse_01, 2: parse_02, 3: parse_03, 4: parse_04, 5: parse_05,
    6: parse_06, 7: parse_07, 8: parse_08, 9: parse_09, 10: parse_10,
    11: parse_11, 12: parse_12, 13: parse_13, 14: parse_14, 15: parse_15,
    16: parse_16,
}

# ---------------------------------------------------------------------------
# MANIFEST — the exact raw record set each report must yield (fail loudly).
# ---------------------------------------------------------------------------

MANIFEST: dict[int, list[str]] = {
    1: ["L-01", "L-02", "L-03", "L-05", "L-06", "P-01", "P-02", "P-03", "P-04",
        "C-01", "C-02", "C-03", "C-04", "M-01", "M-02", "M-03", "M-04", "M-05",
        "T-01", "T-02", "T-03", "T-05", "E-01", "K-01", "K-02", "K-03", "K-04",
        "K-05", "K-06", "R-01", "R-02", "R-03", "R-04", "R-05", "R-06", "R-07",
        "S-01", "S-02"],
    2: ["A1", "A2", "A3", "A4", "A5", "A6", "A7", "A8", "A9",
        "B1", "B2", "B3", "B4", "B5", "B7", "B8",
        "C1", "C2", "C3", "C4", "C5", "C6", "C7", "C8", "C9", "C10", "C11",
        "C12", "C13", "D1", "D2", "D4", "E1", "E2", "E3", "E4", "E8"],
    3: ["M1", "M2", "M3", "M4", "M5", "M6", "M7", "M8", "M9", "M10",
        "A", "B", "R1", "R2", "R3", "R4", "V1", "V2", "V3", "V4",
        "B1", "B2", "B3", "B4", "§3.6", "§3.7", "§3.9", "5.1", "5.2"],
    4: ["B-1", "B-2", "B-3", "A-1", "A-3", "A-4", "A-5", "D-6", "D-8", "B-6",
        "P-06", "P-12", "A-6", "A-7", "A-8", "A-9", "D-4", "D-5", "D-7",
        "G-1/2", "G-3", "P-17", "C-2b", "D-9", "D-10"],
    5: [f"F{i}" for i in range(1, 23)],
    6: ["#" + str(i) for i in range(1, 35)],
    7: [f"F-{i}" for i in range(1, 15)] + [f"N-{i}" for i in range(1, 7)],
    8: ["D-01", "D-02", "D-03", "D-04", "D-05", "D-07", "D-10", "D-11", "D-13",
        "D-14", "D-15", "D-16", "D-18", "D-21", "D-22", "D-23", "D-24", "D-30",
        "D-31", "D-33", "D-34", "D-35", "D-40"],
    9: [f"D{i}" for i in list(range(1, 23)) + [23, 24]],
    10: ["P0-browser", "P0-latex", "P1-sdk", "P1-vscode", "P1-gha", "P1-storybook",
         "P2-containers", "P2-simulation", "P2-storybook", "P2-codegen",
         "P2-recorder", "P2-script", "P2-drizzle", "P2-identity",
         "P3-console", "P3-enterprise", "P3-function", "P3-stats", "P3-posts",
         "P3-services", "PLATFORM"],
    11: [f"#{i}" for i in range(1, 16)] + ["map-1", "map-5", "map-6", "map-7",
         "map-11", "map-13", "map-14", "map-16", "map-17", "map-21", "map-22"],
    12: ["H0"] + [f"H{i}" for i in range(1, 40)],
    13: ["C-1", "C-2", "C-3", "C-4", "C-7", "C-8", "C-9",
         "T-1", "T-2", "T-3", "T-4", "T-5", "T-6", "T-7", "T-9", "T-10", "T-11",
         "T-12", "E-1", "E-2", "E-3", "E-4", "E-5", "E-6"],
    14: [f"TD-{i:02d}" for i in range(1, 29)],
    15: [f"EV-{i}" for i in range(1, 11)],
    16: [f"C{i}" for i in range(1, 7)],
}

# ---------------------------------------------------------------------------
# Curation: fold raw records into existing seed rows (aliases only).
# ---------------------------------------------------------------------------

FOLD_TO_SEED: dict[str, str] = {
    # cross-report same-root-cause folds into immutable seed rows
    "01:M-01": "AI-M1",
    "01:K-06": "AI-M1",          # native compaction 不传 webSocket="session"（与 M-01 同根）
    "08:D-23": "PLUG-M02",       # session.http/ws hook 无 dispatch 站点 = 01:M-02
    "03:R1": "AI-M1",            # openai-responses 默认 WS 通道 = M1
    "03:R2": "AI-M5",            # xai-responses 适配 = M5
    "03:V4": "AI-M3",            # google interactions = M3
    "03:B1": "AI-M10",           # SigV4 凭证链 = M10
    "03:§3.7": "AI-COPILOT-C1",  # copilot responses lane（16:C1 升 S1）
    "05:F10": "VCS-3",           # /api/vcs/diff 形状 = 11:#3
    "11:#4": "VCS-4",            # worktree.remove = 09:D5
    "07:F-1": "CLI-K1",          # serve/hostname = K-1
    "07:F-5": "CLI-K2",          # upgrade 方法集/通道 = K-2
    "08:D-15": "TOOL-C2",        # shell.create.before 未派发 = 02:C2（seed）
    "10:P0-browser": "SURF-BROWSER",
    "10:P0-latex": "SURF-LATEX-MERMAID",
    "10:PLATFORM": "SURF-PLATFORM",
}

# ---------------------------------------------------------------------------
# Curation: new canonical rows. GROUPS maps canonical raw key -> extra alias
# raw keys folded into the same new row. NEW_SPEC gives id + cluster
# (severity/title/etc. come from the parsed record unless overridden here).
# ---------------------------------------------------------------------------

GROUPS: dict[str, list[str]] = {
    "01:R-05": ["05:F11"],                      # title "" 再生（PATCH/清除 → LLM 重命名）
    "03:M6": ["03:B", "03:B3"],                  # bedrock-mantle messages 路由（§3.1:B / §3.5:B3 同根）
    "03:M7": ["03:R3", "03:R4"],                # gateway-protocol（路由选项/元数据）
    "05:F12": ["15:EV-4", "15:EV-8", "15:EV-9"],  # 公共 SSE 事件集合（4+1 多发）
    "09:D14": ["15:EV-7"],                      # project.updated 载荷字段
    "11:#5": ["01:P-03", "02:A6"],                                # image normalize/photon
    "11:#6": ["01:C-01", "11:map-13"],                            # reference 物化（repo.clone）
    "11:#8": ["11:map-14", "11:map-22"],                          # snapshot 行为分歧
    "11:#9": ["11:map-16", "11:map-17", "11:map-21"],             # vcs/git 细节
    "11:#10": ["11:map-11"],                                      # modal 动态模型
    "11:#14": ["09:D13"],                                         # workspace 生命周期
    "11:#15": ["11:map-5", "11:map-6", "11:map-7"],               # environment 执行平面
    "13:C-9": ["13:T-2", "14:TD-24"],           # new-session-location 子系统
}

# raw key -> (new ledger id, cluster); singletons not listed get an
# auto-derived id from their report prefix and the report's default cluster.
NEW_SPEC: dict[str, tuple[str, str]] = {
    # report 01 (session/runner) — RUNTIME unless noted
    "01:L-02": ("RUNTIME-L02", "RUNTIME"),
    "01:L-03": ("RUNTIME-L03", "RUNTIME"),
    "01:L-05": ("RUNTIME-L05", "RUNTIME"),
    "01:L-06": ("RUNTIME-L06", "RUNTIME"),
    "01:P-04": ("RUNTIME-P04", "RUNTIME"),
    "01:C-02": ("RUNTIME-C02", "RUNTIME"),
    "01:C-03": ("RUNTIME-C03", "RUNTIME"),
    "01:C-04": ("RUNTIME-C04", "RUNTIME"),
    "01:M-03": ("AI-MR03", "AI"),
    "01:M-04": ("AI-MR04", "AI"),
    "01:M-05": ("AI-MR05", "AI"),
    "01:T-01": ("RUNTIME-T01", "RUNTIME"),
    "01:T-02": ("RUNTIME-T02", "RUNTIME"),
    "01:T-03": ("RUNTIME-T03", "RUNTIME"),
    "01:T-05": ("RUNTIME-T05", "RUNTIME"),
    "01:E-01": ("RUNTIME-E01", "RUNTIME"),
    "01:K-02": ("RUNTIME-K02", "RUNTIME"),
    "01:K-03": ("RUNTIME-K03", "RUNTIME"),
    "01:K-04": ("RUNTIME-K04", "RUNTIME"),
    "01:K-05": ("RUNTIME-K05", "RUNTIME"),
    "01:R-01": ("RUNTIME-R01", "RUNTIME"),
    "01:R-02": ("RUNTIME-R02", "RUNTIME"),
    "01:R-03": ("RUNTIME-R03", "RUNTIME"),
    "01:R-04": ("RUNTIME-R04", "RUNTIME"),
    "01:R-05": ("RUNTIME-R05", "RUNTIME"),
    "01:R-06": ("RUNTIME-R06", "RUNTIME"),
    "01:R-07": ("RUNTIME-R07", "RUNTIME"),
    "01:S-02": ("RUNTIME-S02", "RUNTIME"),
    # report 02 (tools/exec)
    "02:A3": ("TOOL-A3", "TOOLS-EXEC"),
    "02:A4": ("TOOL-A4", "TOOLS-EXEC"),
    "02:A5": ("TOOL-A5", "TOOLS-EXEC"),
    "02:A7": ("TOOL-A7", "TOOLS-EXEC"),
    "02:A8": ("TOOL-A8", "TOOLS-EXEC"),
    "02:A9": ("TOOL-A9", "TOOLS-EXEC"),
    "02:B1": ("TOOL-B1", "TOOLS-EXEC"),
    "02:B2": ("TOOL-B2", "TOOLS-EXEC"),
    "02:B3": ("TOOL-B3", "TOOLS-EXEC"),
    "02:B4": ("TOOL-B4", "TOOLS-EXEC"),
    "02:B5": ("TOOL-B5", "TOOLS-EXEC"),
    "02:B7": ("TOOL-B7", "TOOLS-EXEC"),
    "02:B8": ("TOOL-B8", "TOOLS-EXEC"),
    "02:C2": ("TOOL-C2", "TOOLS-EXEC"),
    "02:C6": ("TOOL-C6", "TOOLS-EXEC"),
    "02:C7": ("TOOL-C7", "TOOLS-EXEC"),
    "02:C8": ("TOOL-C8", "TOOLS-EXEC"),
    "02:C9": ("TOOL-C9", "TOOLS-EXEC"),
    "02:C10": ("TOOL-C10", "TOOLS-EXEC"),
    "02:C11": ("TOOL-C11", "TOOLS-EXEC"),
    "02:C12": ("TOOL-C12", "TOOLS-EXEC"),
    "02:C13": ("TOOL-C13", "TOOLS-EXEC"),
    "02:D1": ("TOOL-D1", "TOOLS-EXEC"),
    "02:D2": ("TOOL-D2", "TOOLS-EXEC"),
    "02:D4": ("TOOL-D4", "TOOLS-EXEC"),
    "02:E1": ("TOOL-E1", "TOOLS-EXEC"),
    "02:E2": ("TOOL-E2", "TOOLS-EXEC"),
    "02:E3": ("TOOL-E3", "TOOLS-EXEC"),
    "02:E4": ("TOOL-E4", "TOOLS-EXEC"),
    "02:E8": ("TOOL-E8", "TOOLS-EXEC"),
    # report 03 (ai)
    "03:A": ("AI-ANTHROPIC-A", "AI"),
    "03:M6": ("AI-M6", "AI"),
    "03:M7": ("AI-M7", "AI"),
    "03:M8": ("AI-M8", "AI"),
    "03:M9": ("AI-M9", "AI"),
    "03:V1": ("AI-V1", "AI"),
    "03:B4": ("AI-B4", "AI"),
    "03:5.2": ("AI-ABORT", "AI"),
    # report 04 (provider/auth) — all CRED
    "04:A-6": ("CRED-A6", "CRED"),
    "04:A-7": ("CRED-A7", "CRED"),
    "04:A-8": ("CRED-A8", "CRED"),
    "04:A-9": ("CRED-A9", "CRED"),
    "04:D-4": ("CRED-D4", "CRED"),
    "04:D-5": ("CRED-D5", "CRED"),
    "04:D-7": ("CRED-D7", "CRED"),
    "04:G-1/2": ("CRED-G12", "CRED"),
    "04:G-3": ("CRED-G3", "CRED"),
    "04:P-17": ("CRED-P17", "CRED"),
    "04:C-2b": ("CRED-C2B", "CRED"),
    "04:D-9": ("CRED-D9", "CRED"),
    "04:D-10": ("CRED-D10", "CRED"),
    # report 05 (api/wire)
    "05:F5": ("HTTP-F5", "WIRE"),
    "05:F6": ("HTTP-F6", "WIRE"),
    "05:F7": ("HTTP-F7", "WIRE"),
    "05:F8": ("HTTP-F8", "WIRE"),
    "05:F9": ("HTTP-F9", "WIRE"),
    "05:F12": ("HTTP-F12", "WIRE"),
    "05:F13": ("HTTP-F13", "WIRE"),
    "05:F14": ("HTTP-F14", "WIRE"),
    "05:F15": ("HTTP-F15", "WIRE"),
    "05:F16": ("HTTP-F16", "WIRE"),
    "05:F17": ("HTTP-F17", "WIRE"),
    "05:F18": ("HTTP-F18", "WIRE"),
    "05:F19": ("HTTP-F19", "WIRE"),
    "05:F20": ("HTTP-F20", "WIRE"),
    "05:F21": ("HTTP-F21", "WIRE"),
    "05:F22": ("HTTP-F22", "WIRE"),
    # report 06 (tui)
    **{f"06:#{i}": (f"TUI-{i}", "TUI") for i in range(18, 35)},
    # report 07 (cli/config)
    **{f"07:F-{i}": (f"CLI-F{i}", "CONFIG") for i in range(1, 15)},
    "07:N-5": ("CLI-N5", "CONFIG"),
    "07:N-6": ("CLI-N6", "CONFIG"),
    # report 08 (plugin)
    "08:D-02": ("PLUG-D02", "PLUGIN"),
    "08:D-03": ("PLUG-D03", "PLUGIN"),
    "08:D-04": ("PLUG-D04", "PLUGIN"),
    "08:D-05": ("PLUG-D05", "PLUGIN"),
    "08:D-16": ("PLUG-D16", "PLUGIN"),
    "08:D-22": ("PLUG-D22", "PLUGIN"),
    "08:D-24": ("PLUG-D24", "PLUGIN"),
    "08:D-34": ("PLUG-D34", "PLUGIN"),
    "08:D-40": ("PLUG-D40", "PLUGIN"),
    # report 09 (db)
    **{f"09:D{i}": (f"DB-D{i}", "DB") for i in (7, 8, 9, 10, 11, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24)},
    # report 10 (surfaces)
    "10:P1-sdk": ("SURF-P1-SDK", "SURFACES"),
    "10:P1-vscode": ("SURF-P1-VSCODE", "SURFACES"),
    "10:P1-gha": ("SURF-P1-GHA", "SURFACES"),
    "10:P1-storybook": ("SURF-P1-STORYBOOK", "SURFACES"),
    "10:P2-containers": ("SURF-P2-CONTAINERS", "SURFACES"),
    "10:P2-simulation": ("SURF-P2-SIMULATION", "SURFACES"),
    "10:P2-storybook": ("SURF-P2-STORYBOOK", "SURFACES"),
    "10:P2-codegen": ("SURF-P2-CODEGEN", "SURFACES"),
    "10:P2-recorder": ("SURF-P2-RECORDER", "SURFACES"),
    "10:P2-script": ("SURF-P2-SCRIPT", "SURFACES"),
    "10:P2-drizzle": ("SURF-P2-DRIZZLE", "SURFACES"),
    "10:P2-identity": ("SURF-P2-IDENTITY", "SURFACES"),
    "10:P3-console": ("SURF-P3-CONSOLE", "SURFACES"),
    "10:P3-enterprise": ("SURF-P3-ENTERPRISE", "SURFACES"),
    "10:P3-function": ("SURF-P3-FUNCTION", "SURFACES"),
    "10:P3-stats": ("SURF-P3-STATS", "SURFACES"),
    "10:P3-posts": ("SURF-P3-POSTS", "SURFACES"),
    "10:P3-services": ("SURF-P3-SERVICES", "SURFACES"),
    # report 11 (core misc)
    "11:#5": ("TOOL-IMAGE", "TOOLS-EXEC"),
    "11:#6": ("RUNTIME-REFERENCE", "RUNTIME"),
    "11:#7": ("TOOL-RETENTION", "TOOLS-EXEC"),
    "11:#8": ("TOOL-SNAPSHOT", "TOOLS-EXEC"),
    "11:#9": ("WIRE-VCS-DETAIL", "WIRE"),
    "11:#10": ("AI-MODAL", "AI"),
    "11:#11": ("TOOL-WEBSEARCH", "TOOLS-EXEC"),
    "11:#12": ("TOOL-CODEMODE-PINNED", "TOOLS-EXEC"),
    "11:#13": ("CRED-WELLKNOWN-LOGIN", "CRED"),
    "11:#14": ("DB-WORKSPACE", "DB"),
    "11:#15": ("RUNTIME-ENVIRONMENT", "RUNTIME"),
    "11:map-1": ("TOOL-CODEMODE-BUDGET", "TOOLS-EXEC"),
    # report 12 (http payloads)
    **{f"12:H{i}": (f"HTTP-H{i}", "WIRE") for i in range(7, 40)},
    # report 13 (config/env)
    "13:C-2": ("CONFIG-C2", "CONFIG"),
    "13:C-3": ("CONFIG-C3", "CONFIG"),
    "13:C-4": ("CONFIG-C4", "CONFIG"),
    "13:C-9": ("CONFIG-C9", "CONFIG"),
    "13:T-5": ("CONFIG-T5", "CONFIG"),
    "13:T-6": ("CONFIG-T6", "CONFIG"),
    "13:T-7": ("CONFIG-T7", "CONFIG"),
    "13:T-9": ("CONFIG-T9", "CONFIG"),
    "13:T-10": ("CONFIG-T10", "CONFIG"),
    "13:T-11": ("CONFIG-T11", "CONFIG"),
    "13:T-12": ("CONFIG-T12", "CONFIG"),
    **{f"13:E-{i}": (f"CONFIG-E{i}", "CONFIG") for i in range(1, 7)},
    # report 14 (tui deep)
    **{f"14:TD-{i:02d}": (f"TUI-TD{i:02d}", "TUI") for i in
       (6, 8, 9, 10, 12, 13, 14, 15, 20, 21, 22, 26, 27, 28)},
    # report 15 (event payloads)
    "15:EV-2": ("EV-2", "WIRE"),
    "15:EV-3": ("EV-3", "WIRE"),
    "15:EV-5": ("EV-5", "WIRE"),
    "15:EV-6": ("EV-6", "WIRE"),
    "15:EV-10": ("EV-10", "WIRE"),
    # report 16 (copilot/media)
    "16:C3": ("AI-COPILOT-C3", "AI"),
    "16:C4": ("AI-COPILOT-C4", "AI"),
    "16:C5": ("AI-COPILOT-C5", "AI"),
    "16:C6": ("AI-COPILOT-C6", "AI"),
}

# Curated per-row overrides for quality: raw key -> dict of field overrides.
OVERRIDES: dict[str, dict] = {
    # symptom-code overrides (fingerprint middle segment + test case slug)
    "01:L-05": {"code": "move-destination-validation"},
    "01:R-01": {"code": "title-race-retry",
                "notes": "EXTRA/有益增量（ROADMAP wave F #27 有意增强）：登记备查；如裁定永久分歧转 accepted。"},
    "02:B2": {"code": "read-binary-check"},
    "02:B3": {"code": "read-suggestion-order"},
    "02:C12": {"code": "rg-exit-codes"},
    "02:C8": {"code": "timeout-term-grace"},
    "02:D1": {"code": "structured-output-dropped"},
    "02:E4": {"code": "patch-parent-dirs"},
    "03:B4": {"code": "bedrock-test-coverage"},
    "04:G-1/2": {"code": "cli-interactive-login"},
    "04:P-17": {"code": "oauth-callback-page"},
    "07:F-9": {"code": "api-query-encoding"},
    "07:N-6": {"code": "otel-observability"},
    "08:D-02": {"code": "hot-reload-unwired"},
    "08:D-05": {"code": "v1-plugin-tolerated"},
    "08:D-34": {"code": "mcp-credential-reconnect"},
    "08:D-40": {"code": "integration-providers-missing"},
    "10:P1-vscode": {"code": "vscode-extension"},
    "12:H28": {"code": "notfound-message-envelope"},
    "06:#31": {"code": "toast-activate-chord"},
    "14:TD-06": {"code": "scroll-anchor-restore"},
    "14:TD-15": {"code": "prompt-autosubmit-ready"},
    "14:TD-22": {"code": "dialog-experiments"},
    "01:R-02": {"notes": "EXTRA：wire 枚举超集（多 Superseded reason）；登记备查。"},
    "01:R-03": {"notes": "EXTRA/有益增量（ROADMAP B4-H2 有意落地）：登记备查。"},
    "01:R-06": {"notes": "EXTRA/有益增量（ROADMAP GATE4/DETERM）：SSE 顺序确定化；登记备查。"},
    "01:S-02": {"notes": "EXTRA：完成文本过滤比 ref 更严格（与 job body 一致）；登记备查。"},
    "01:E-01": {"notes": "理论性差异（两条件不可能同时成立）；列出备查。"},
    "09:D23": {"notes": "EXTRA/有益加固（跨进程双检/WAL 重试/NULL 容错/bun 互操作测试）：正向差异，登记备查。"},
    "15:EV-10": {"notes": "跨 lane 纠错：双侧 v2.0.24 均不发布 command.executed，非公共流缺口；登记备查（修正 01 报告映射表表述）。"},
    "13:C-4": {"notes": "信息项：双侧死键（share/username/lsp/instructions/enterprise.url），登记备查。"},
    "05:F21": {"notes": "良性扩展：服务器忽略 Last-Event-ID 头，行为与 v2 一致；登记备查。"},
    "06:#31": {"notes": "EXTRA（已登记 PORT ADDITION）：toast.activate 键盘弦；登记备查。"},
    "03:V1": {"status": "needs-runtime"},
    "03:5.2": {"notes": "观察项：语义近似等价，缺专门 parity 测试。"},
    "10:P3-console": {"severity": "S3"}, "10:P3-enterprise": {"severity": "S3"},
    "10:P3-function": {"severity": "S3"}, "10:P3-stats": {"severity": "S3"},
    "10:P3-posts": {"severity": "S3"}, "10:P3-services": {"severity": "S3"},
    "10:P2-containers": {"severity": "S2"}, "10:P2-simulation": {"severity": "S2"},
    "10:P2-storybook": {"severity": "S2"}, "10:P2-codegen": {"severity": "S2"},
    "10:P2-recorder": {"severity": "S2"}, "10:P2-script": {"severity": "S2"},
    "10:P2-drizzle": {"severity": "S2"}, "10:P2-identity": {"severity": "S2"},
    "10:P1-storybook": {"severity": "S2"},
}

# Surface rows use a packaging/runtime verification step instead of a rust test.
SURFACE_ROWS_PREFIX = "SURF-"
_SEEN_FP: set[str] = set()


def default_wiring() -> dict:
    return {
        "implementation": False,
        "production_call_site": False,
        "config_env_consumer": "n/a",
        "event_publish_subscribe": "n/a",
        "db_read_write_migration": "n/a",
        "error_timeout_cancel": "n/a",
        "cross_process_restart": "n/a",
        "tui_cli_api_surface": False,
        "regression_or_differential_test": False,
        "ledger_evidence": False,
    }


_RUST_FILE_INDEX: dict[str, list[str]] | None = None


def rust_file_index() -> dict[str, list[str]]:
    """relpath-or-stem -> [crate, ...] built from crates/*/src (deterministic, cached)."""
    global _RUST_FILE_INDEX
    if _RUST_FILE_INDEX is None:
        idx: dict[str, list[str]] = {}
        crates_dir = ROOT / "crates"
        if crates_dir.is_dir():
            for crate in sorted(p.name for p in crates_dir.iterdir() if p.is_dir()):
                src = crates_dir / crate / "src"
                if not src.is_dir():
                    continue
                for f in sorted(src.rglob("*.rs")):
                    rel = str(f.relative_to(crates_dir / crate / "src"))
                    for key in (rel, f.name):
                        idx.setdefault(key, [])
                        if crate not in idx[key]:
                            idx[key].append(crate)
        _RUST_FILE_INDEX = idx
    return _RUST_FILE_INDEX


def crate_for(cluster: str, rusts: list[str]) -> str:
    idx = rust_file_index()
    prefs = CLUSTER_CRATE_PREF[cluster]
    cands: list[str] = []
    for p in rusts:
        file = p.split(":", 1)[0].removeprefix("crates/")
        file = re.sub(r"^oc-[\w-]+/src/", "", file)
        for key in (file, file.rsplit("/", 1)[-1]):
            for crate in idx.get(key, []):
                if crate not in cands:
                    cands.append(crate)
            if cands:
                break
    for pref in prefs:
        if pref in cands:
            return pref
    if cands:
        return sorted(cands)[0]
    return prefs[0]


def build_new_row(rec: Rec, new_id: str, cluster: str) -> dict:
    domain = CLUSTER_DOMAIN[cluster]
    ov0 = OVERRIDES.get(rec.key, {})
    code = ov0.get("code") or slug_code(rec.title, new_id)
    # enrich path lists from the full record text
    extra_refs, extra_rusts = split_paths(
        " ".join([rec.expected, rec.actual, rec.impact, rec.body]))
    refs = [norm_path_item(x) for x in rec.refs]
    rusts = [norm_path_item(x) for x in rec.rusts]
    refs = list(dict.fromkeys(refs + extra_refs))
    rusts = list(dict.fromkeys(rusts + extra_rusts))
    refs = [r for r in refs if r not in ("无", "—", "n/a", "")]
    rusts = [r for r in rusts if r not in ("无", "—", "n/a", "")]
    rec = Rec(rec.report, rec.rid, title=rec.title, sev=rec.sev, typ=rec.typ,
              known=rec.known, expected=rec.expected, actual=rec.actual,
              impact=rec.impact, refs=refs, rusts=rusts, body=rec.body)
    crate = crate_for(cluster, rec.rusts)
    case = re.sub(r"[^a-z0-9]+", "_", code).strip("_") or new_id.lower()
    unverified = any(m in rec.text() for m in UNVERIFIED_MARKERS)
    ov = OVERRIDES.get(rec.key, {})
    status = ov.get("status", "needs-runtime" if unverified else "open")
    severity = ov.get("severity", rec.sev)

    if new_id.startswith(SURFACE_ROWS_PREFIX):
        repro = ("runtime: 构建发布产物并核对交付渠道（npm registry / VS Code marketplace / "
                 "GitHub Action marketplace / AUR / Homebrew）与平台矩阵（linux-x64/win/aarch64）")
        test = f"tools/parity/surface_checks.py::{case}"
    elif status == "needs-runtime":
        repro = (f"runtime: 在真实运行环境复现「{clean(rec.title, 60)}」，"
                 "记录 ref/rust 双侧输出作为 red artifact")
        test = f"crates/{crate}/tests/{CLUSTER_TESTFILE[cluster]}::{case}"
    else:
        repro = f"cargo nextest run -p {crate} -E 'test({case})'"
        test = f"crates/{crate}/tests/{CLUSTER_TESTFILE[cluster]}::{case}"

    first_file = "missing"
    for p in rec.rusts:
        first_file = p.split(":", 1)[0].removeprefix("crates/")
        break
    if first_file == "missing":
        for p in rec.refs:
            first_file = p.split(":", 1)[0]
            break
    fingerprint = f"{domain}|{code}|{first_file}"
    global _SEEN_FP
    if fingerprint in _SEEN_FP:  # deterministic disambiguation, never silently merge
        fingerprint = f"{fingerprint}|{new_id.lower()}"
    _SEEN_FP.add(fingerprint)

    notes = f"导入自 {REPORT_FILES[rec.report]}（{rec.rid}）；报告标注：{rec.known or 'NEW'}。"
    if status == "needs-runtime":
        notes += (" 运行时计划：静态无法确认（UNVERIFIED/静态推断），需先在运行时复现并留存"
                  " red artifact，再转入 reproduced/open。")
    notes += " " + ov.get("notes", "") if ov.get("notes") else ""

    title = clean(rec.title, 90) or new_id
    row = {
        "id": new_id,
        "source_id": rec.key,
        "aliases": [],
        "fingerprint": fingerprint,
        "domain": domain,
        "cluster": cluster,
        "severity": severity,
        "status": status,
        "title": title,
        "symptom": clean(rec.impact or rec.title, 200) or title,
        "expected": clean(rec.expected, 400) or "对齐 v2（见 ref 证据）",
        "actual": clean(rec.actual, 400) or "见 rust 证据",
        "impact": clean(rec.impact, 300) or clean(rec.title, 200),
        "ref": rec.refs or ["（见扫描报告原文）"],
        "rust": rec.rusts or ["（无对应实现/见报告）"],        "repro": repro,
        "test": test,
        "owner": "",
        "upstream": "2.0.24",
        "wiring": default_wiring(),
        "evidence": [],
        "merge_commit": "",
        "merged_at": "",
        "notes": notes.strip(),
        "created": DATE,
        "updated": DATE,
    }
    return row


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true", help="verify only; do not write")
    args = ap.parse_args()

    # ---- parse all reports, verify against MANIFEST (fail loudly) ----
    records: dict[str, Rec] = {}
    for rep, fname in sorted(REPORT_FILES.items()):
        text = (SCAN / fname).read_text(encoding="utf-8")
        recs = PARSERS[rep](text)
        got = sorted(r.rid for r in recs)
        want = sorted(MANIFEST[rep])
        if got != want:
            missing = sorted(set(want) - set(got))
            extra = sorted(set(got) - set(want))
            print(f"FATAL {fname}: parsed {len(got)} records, manifest expects {len(want)}")
            if missing:
                print(f"  missing from parse: {missing}")
            if extra:
                print(f"  unexpected from parse: {extra}")
            return 1
        for r in recs:
            if r.key in records:
                print(f"FATAL duplicate raw key {r.key}")
                return 1
            records[r.key] = r
    total_raw = len(records)

    # ---- load seeds ----
    if not LEDGER.is_file():
        print(f"FATAL ledger not found: {LEDGER}")
        return 1
    seed_lines = [ln for ln in LEDGER.read_text(encoding="utf-8").splitlines() if ln.strip()]
    all_rows = [json.loads(ln) for ln in seed_lines]
    # Idempotency: rows generated by this importer carry a "导入自 " notes
    # prefix; strip them so re-running reproduces the same output from the
    # immutable 114-row seed baseline.
    seeds = [r for r in all_rows if not (r.get("notes") or "").startswith("导入自 ")]
    if len(seeds) != 114:
        print(f"FATAL expected 114 seed rows after stripping importer rows, got {len(seeds)}")
        return 1
    by_id = {s["id"]: s for s in seeds}
    for s in seeds:  # never let a new row collide with a seed fingerprint
        _SEEN_FP.add(s.get("fingerprint", ""))
    seed_raw_refs: dict[str, str] = {}  # raw key ("04:B-1") -> seed id
    for s in seeds:
        for ref in [s.get("source_id", "")] + list(s.get("aliases") or []):
            if ref and ref != "n/a" and ":" in ref:
                seed_raw_refs.setdefault(ref, s["id"])

    # ---- classify every raw record ----
    new_rows: list[dict] = []
    fold_seed: dict[str, str] = {}       # raw key -> seed id (alias addition)
    fold_new: dict[str, str] = {}        # raw key -> new row id (alias addition)
    stats = {"per_report": {r: {"raw": 0, "seed": 0, "fold_seed": 0, "new": 0, "fold_new": 0}
                            for r in sorted(REPORT_FILES)}}

    # pass 1: determine canonical new rows
    canonical: dict[str, str] = {}  # raw key -> new id
    for key in sorted(records):
        if key in FOLD_TO_SEED or key in seed_raw_refs:
            continue
        if key in GROUPS or key in NEW_SPEC:
            nid = NEW_SPEC.get(key, (key.split(":", 1)[1].replace("#", "N").replace("map-", "M"), "RUNTIME"))[0]
            canonical[key] = nid
    # every GROUPS member must be canonical or handled
    for ckey, members in GROUPS.items():
        if ckey not in canonical:
            print(f"FATAL group canonical {ckey} not a new-row key")
            return 1
        for m in members:
            if m in canonical or m in FOLD_TO_SEED or m in seed_raw_refs:
                print(f"FATAL group member {m} of {ckey} conflicts (already canonical/seed)")
                return 1

    # pass 2: build rows / assign folds
    for key in sorted(records):
        rep = int(key.split(":", 1)[0])
        st = stats["per_report"][rep]
        st["raw"] += 1
        if key in FOLD_TO_SEED:
            fold_seed[key] = FOLD_TO_SEED[key]
            st["fold_seed"] += 1
            continue
        if key in seed_raw_refs:
            st["seed"] += 1
            continue
        if key in canonical:
            rec = records[key]
            default_cluster = NEW_SPEC.get(key, ("", "RUNTIME"))[1] if key in NEW_SPEC else "RUNTIME"
            row = build_new_row(rec, canonical[key], default_cluster)
            new_rows.append(row)
            st["new"] += 1
            continue
        # must belong to a group
        target = None
        for ckey, members in GROUPS.items():
            if key in members:
                target = ckey
                break
        if target is None:
            print(f"FATAL raw record {key} unaccounted (not seed/fold/canonical/group)")
            return 1
        fold_new[key] = canonical[target]
        st["fold_new"] += 1

    # apply alias additions to seeds (deterministic order)
    for key in sorted(fold_seed):
        sid = fold_seed[key]
        if key not in (by_id[sid].get("aliases") or []) and key != by_id[sid].get("source_id"):
            by_id[sid].setdefault("aliases", []).append(key)
    # apply alias additions to new rows
    by_new_id = {r["id"]: r for r in new_rows}
    for key in sorted(fold_new):
        nid = fold_new[key]
        if key not in by_new_id[nid]["aliases"]:
            by_new_id[nid]["aliases"].append(key)

    # ---- merge & order: seeds keep original order; new rows sorted (cluster, id) ----
    out_rows: list[dict] = []
    for s in seeds:
        out_rows.append(by_id[s["id"]])
    new_rows.sort(key=lambda r: (r["cluster"], r["id"]))
    out_rows.extend(new_rows)

    # ---- invariants ----
    errors: list[str] = []
    ids = [r["id"] for r in out_rows]
    if len(ids) != len(set(ids)):
        dup = sorted({i for i in ids if ids.count(i) > 1})
        errors.append(f"duplicate ledger ids: {dup}")
    fps = [r["fingerprint"] for r in out_rows]
    fp_dup = sorted({f for f in fps if fps.count(f) > 1})
    if fp_dup:
        errors.append(f"duplicate fingerprints: {fp_dup}")
    # zero-orphan check
    covered: set[str] = set()
    for r in out_rows:
        covered.add(r.get("source_id", ""))
        covered.update(r.get("aliases") or [])
    orphans = sorted(k for k in records if k not in covered)
    if orphans:
        errors.append(f"orphan raw ids: {orphans}")
    # group aliases actually landed
    for ckey, members in GROUPS.items():
        nid = canonical[ckey]
        for m in members:
            if m not in (by_new_id[nid]["aliases"] or []):
                errors.append(f"group member {m} missing from aliases of {nid}")

    if errors:
        print("FATAL invariants failed:")
        for e in errors:
            print("  -", e)
        return 1

    # ---- write ----
    if not args.check:
        payload = "\n".join(json.dumps(r, ensure_ascii=False) for r in out_rows) + "\n"
        LEDGER.write_text(payload, encoding="utf-8")

    # ---- reconciliation stats ----
    n_seed = len(seeds)
    n_new = len(new_rows)
    n_fold_seed = len(fold_seed)
    n_fold_new = len(fold_new)
    n_seedref = sum(1 for k in records if k in seed_raw_refs and k not in FOLD_TO_SEED)
    needs_runtime = sum(1 for r in out_rows if r["status"] == "needs-runtime")
    print(f"raw_records={total_raw} (manifest total)")
    print(f"ledger_rows={n_seed + n_new} (seeds={n_seed} immutable + new={n_new})")
    print(f"accounting: seed_covered={n_seedref} folded_into_seed={n_fold_seed} "
          f"new_canonical={n_new} folded_into_new={n_fold_new} "
          f"total={n_seedref + n_fold_seed + n_new + n_fold_new}")
    print(f"needs_runtime_rows={needs_runtime}")
    print("\nper report (raw | master | seed-covered | fold→seed | new | fold→new):")
    second_round = 0
    for rep in sorted(REPORT_FILES):
        st = stats["per_report"][rep]
        master = MASTER_COUNTS.get(rep, "")
        print(f"  {rep:02d} {REPORT_FILES[rep]:26} raw={st['raw']:3} master={str(master):>4} "
              f"seed={st['seed']:3} fold_seed={st['fold_seed']} new={st['new']:3} fold_new={st['fold_new']}")
        if rep >= 12:
            second_round += st["raw"]
    print(f"second round (12-16) raw total={second_round} (master 108)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
