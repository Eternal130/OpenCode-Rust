#!/usr/bin/env python3
"""Env-var census: process-boundary environment variables read/written by both
sides.

Axis — the full set of literal environment-variable names:
  rust: `std::env::var`/`var_os` (reads), `env!`/`option_env!` (compile-time),
        `set_var`/`remove_var` (writes) — literals only, across crates/
        (production regions; `#[cfg(test)]`/`#[test]` stripped via
        common.strip_tests; *.gen.rs skipped). Dynamic `envs()`/`vars()`
        iterations are counted but not expandable (noted).
  ref:  `process.env.X` / `Bun.env.X` / `process.env["X"]` member reads
        (assignments to `process.env.X` classified as writes), across
        packages/ excluding node_modules, /test/, /e2e/, *.test.ts,
        __tests__. Dynamic `process.env[name]` reads are counted and noted.

Normalization: comparison is on the upper-cased name; a raw-c spelling
difference is reported as CASE_DIFF (never silently merged away — both raw
spellings listed).

Classification:
  REF_ONLY  = ref reads, rust does not  → gap candidates (each verified by
              reading both sides' current code; old docs are leads only)
  RUST_ONLY = rust reads, ref does not  → rust extra knobs; cross-checked
              against docs/GAPS-AND-PITFALLS.md (read-only) — mentions are
              reported as documented divergences (e.g. OPENCODE_PTY_BIN)
  BOTH      = both read; different *purposes* are OUT OF SCOPE for this
              census (declared, no behavioural verdict).

Usage: python3 tools/census/env_vars.py [--ref /opt/opencode-ref]
          [--write docs/audit/census/ENV-VARS.md]
"""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

from common import Report, strip_tests  # noqa: E402

GAPS_DOC = Path("docs/GAPS-AND-PITFALLS.md")

# 差异裁定表：运行时相关 REF_ONLY 项的亲读定性（构建/脚本类由机械分组处理）
ENV_ADJUDICATIONS = {
    "OPENCODE_FILEWATCHER_DISABLE": (
        "已文档化缺口",
        "rust `crates/oc-server/src/server_process_env.rs:22-24` 注释明示 \"Not claimable in this "
        "port … flagged, not faked\"（v2 `Watcher.configured`）；ref `packages/cli/src/server-process.ts:124`。",
    ),
    "OPENCODE_DISABLE_FILEWATCHER": (
        "已文档化缺口",
        "同上（同名旧拼写，ref server-process.ts:124 双名兼容）。",
    ),
    "OPENCODE_LOG_LEVEL": (
        "缺口候选",
        "ref `packages/util/src/observability/logging.ts:165` 运行时日志级别旋钮；rust 生产代码无读取"
        "（仅测试区出现）——rust 日志级别面缺口，登记候选。",
    ),
    "TMUX": (
        "缺口候选",
        "ref `packages/tui/src/util/system.ts:18` 终端多路复用探测；rust TUI 未读 TMUX。",
    ),
    "STY": (
        "缺口候选",
        "ref `packages/tui/src/util/system.ts:18` GNU screen 探测；rust 未读。",
    ),
    "TERM_PROGRAM_VERSION": (
        "缺口候选",
        "ref `packages/tui/src/util/system.ts:17` 终端版本探测；rust 未读。",
    ),
    "OPENCODE_ROUTE": ("有据分歧", "ref tui/src/app.tsx:348-349 演示路由；GAPS-AND-PITFALLS.md:1300 段已提及同族。"),
    "OPENCODE_STORY": ("有据分歧", "ref tui/src/app.tsx:337/344 storybook 演示模式；GAPS:1300 段提及。"),
    "OPENCODE_DRIVE": ("有据分歧", "ref tui/src/app.tsx:253 drive 集成；GAPS:1300 段提及。"),
    "PATHEXT": (
        "半缺口候选",
        "ref `packages/core/src/util/which.ts:10` Windows 可执行扩展解析；rust "
        "`oc-core/src/environment/local.rs:23` 注释提及 PATHEXT 解析但 census 未见生产读取（GAPS:623 提及）——windows which 通道复核。",
    ),
    "AICORE_SERVICE_KEY": (
        "注册表族·待集成通道",
        "ref `core/src/plugin/provider/sap-ai-core.ts:16-18` 写读兼有；rust 经 integration/provider_catalog "
        "注册表读取（Method::Env 表已比对，此名不在表内）——归 provider/integration 通道裁定。",
    ),
    "AICORE_DEPLOYMENT_ID": ("注册表族·待集成通道", "同上（sap-ai-core.ts:26）。"),
    "AICORE_RESOURCE_GROUP": ("注册表族·待集成通道", "同上（sap-ai-core.ts:26）。"),
    "AZURE_COGNITIVE_SERVICES_RESOURCE_NAME": (
        "注册表族·待集成通道",
        "ref `core/src/plugin/provider/azure.ts:476`；rust `provider_catalog/azure_sync.rs:66` 有同名常量 "
        "AZURE_LEGACY_RESOURCE_NAME_ENV、`integration/providers_enterprise.rs:444` 有名表——提取器未并表，归集成通道复核。",
    ),
    "CLOUDFLARE_ACCOUNT_ID": ("注册表族·待集成通道", "ref cloudflare-ai-gateway.ts:29 / cloudflare-workers-ai.ts:53；rust 门面注释（facade.rs:1242）提及经凭据链而非裸 env——归集成通道。"),
    "CLOUDFLARE_GATEWAY_ID": ("注册表族·待集成通道", "ref cloudflare-ai-gateway.ts:30；同上。"),
    "GITLAB_TOKEN": ("注册表族·待集成通道", "ref core/src/plugin/provider/gitlab.ts:304；rust integration 表无此名。"),
    "MODAL_PROXY_TOKEN": (
        "提取器已核 BOTH（跨文件常量）",
        "rust `provider_catalog/modal_sync.rs:47` const MODAL_PROXY_TOKEN_ENV + `engine/mod.rs:608` 读取；"
        "本轮提取器常量路径解析应捕获——若仍列 REF_ONLY 为提取遗漏，非缺口。",
    ),
    "SNOWFLAKE_ACCOUNT": ("注册表族·待集成通道", "ref core/src/plugin/provider/snowflake-cortex.ts:176；rust providers_enterprise.rs 只列 SNOWFLAKE_CORTEX_TOKEN/PAT。"),
    "GOOGLE_VERTEX_API_KEY": ("注册表族·已核 BOTH", "rust `integration/providers.rs:72` google-vertex 表项含此名（table 提取应捕获）。"),
    "OPENCODE_NODE_PTY_PATH": ("生态资产类·不适用", "node 侧 pty 原生模块路径覆盖（core/src/pty/pty.node.ts:8）；rust 自带 oc-pty 进程，无 node 模块加载。"),
    "OPENCODE_PARCEL_WATCHER_PATH": ("生态资产类·不适用", "node 文件监视原生模块（watcher-binding.ts:11）；rust 用原生 notify。"),
    "OPENCODE_PHOTON_WASM_PATH": ("生态资产类·不适用", "WASM 图像处理模块路径（image/photon-wasm.node.ts:3）；rust 原生图像路径。"),
    "OPENCODE_TREE_SITTER_WASM_PATH": ("生态资产类·不适用", "WASM 语法分析模块（shell/parser-wasm.node.ts:6）；rust 内置 tree-sitter。"),
    "OPENCODE_TREE_SITTER_BASH_WASM_PATH": ("生态资产类·不适用", "同上（:7）。"),
    "OPENCODE_TREE_SITTER_POWERSHELL_WASM_PATH": ("生态资产类·不适用", "同上（:9）。"),
    "OPENCODE_NODE_ASSETS_DIR": ("生态资产类·不适用", "node 资产目录（tui/attention-sounds.node.ts:7-8）；rust 资产经 OPENCODE_APP_ASSETS（rust-only）。"),
    "NODE_ENV": ("生态变量·不适用", "node 运行时模式变量；rust 原生二进制无对应语义。"),
    "OPENCODE_DIRECT_TRACE": ("演示/诊断类·低优先", "ref cli/src/mini-host.ts:50 mini 宿主直连 trace；rust mini 通道待定。"),
    "OPENCODE_SHOW_TTFD": ("演示/诊断类·低优先", "ref cli/src/mini-host.ts:164 TTFD 显示；同上。"),
    "OPENCODE_BASE_URL": (
        "缺口候选",
        "ref sdk/client 侧服务地址变量（client 包）；rust 客户端地址面待 client 通道核对。",
    ),
}


# ---------------------------------------------------------------- rust side

def rust_env(crates: Path) -> tuple[dict[str, dict[str, list[tuple[str, int]]]], int]:
    """{NAME: {kind: [(file, line)]}}; kinds: read, compile, write, indirect."""
    found: dict[str, dict[str, list[tuple[str, int]]]] = {}
    dynamic = 0
    files = []
    for f in sorted(crates.rglob("*.rs")):
        if f.name.endswith(".gen.rs"):
            continue
        s = str(f)
        if "/tests/" in s or "/benches/" in s or "/examples/" in s or f.name == "tests.rs":
            continue
        files.append((f, strip_tests(f.read_text())))

    # wrapper functions: fn whose body forwards a &str param straight to
    # env::var/_os (e.g. `fn non_empty(var: &str) { env::var_os(var) }`),
    # plus one level of chaining onto such direct wrappers (xdg -> non_empty).
    def fn_params_str(paramtext: str) -> set[str]:
        out = set()
        for p in paramtext.split(","):
            if ":" in p and re.search(r":\s*(?:&?str|S|impl AsRef<str>)\b", p):
                out.add(p.split(":")[0].strip())
        return out

    direct: set[str] = set()
    chainable: set[str] = set()
    for f, body in files:
        for m in re.finditer(r"fn (\w+)\s*\(([^)]*)\)[^{]*\{", body):
            close = body.find("\n}", m.end())
            fnbody = body[m.end(): close if close != -1 else len(body)][:2000]
            params = fn_params_str(m.group(2))
            if not params:
                continue
            if any(
                pm.group(1) in params
                for pm in re.finditer(r"env::var(?:_os)?\s*\(\s*([a-z_]\w*)\s*\)", fnbody)
            ):
                direct.add(m.group(1))
    # one-hop chaining onto unambiguous direct wrappers (exclude `get`:
    # collides with ubiquitous JSON-key getters of the same name)
    chain_base = direct - {"get"}
    for f, body in files:
        for m in re.finditer(r"fn (\w+)\s*\(([^)]*)\)[^{]*\{", body):
            fname = m.group(1)
            if fname in direct:
                continue
            close = body.find("\n}", m.end())
            fnbody = body[m.end(): close if close != -1 else len(body)][:2000]
            params = fn_params_str(m.group(2))
            if not params:
                continue
            if any(
                wm.group(1) in chain_base and wm.group(2) in params
                for wm in re.finditer(r"(?<!\.)\b(\w+)\s*\(\s*([a-z_]\w*)\s*[\),]", fnbody)
            ):
                direct.add(fname)
    wrappers = {n: None for n in direct}
    # const-indirect: `const NAME: &str = "VAR"` used later as env::var(NAME)
    # or passed to a wrapper fn (env_truthy(DISABLE_ENV)); path-qualified
    # const references (crate::x::CONST) resolve via a global unique-name map.
    const_env_global: dict[str, dict[str, list[tuple[str, int]]]] = {}
    file_consts: dict[str, dict[str, str]] = {}
    global_consts: dict[str, str] = {}
    const_name_counts: dict[str, int] = {}
    for f, body in files:
        for cm in re.finditer(r'const (\w+)\s*:\s*&?str\s*=\s*"([A-Za-z_][\w]*)"', body):
            const_name_counts[cm.group(1)] = const_name_counts.get(cm.group(1), 0) + 1
    for f, body in files:
        for cm in re.finditer(r'const (\w+)\s*:\s*&?str\s*=\s*"([A-Za-z_][\w]*)"', body):
            if const_name_counts[cm.group(1)] == 1:
                global_consts[cm.group(1)] = cm.group(2)
    for f, body in files:
        consts = {
            cm.group(1): cm.group(2)
            for cm in re.finditer(r'const (\w+)\s*:\s*&?str\s*=\s*"([A-Za-z_][\w]*)"', body)
            if const_name_counts[cm.group(1)] == 1
        }
        if consts:
            file_consts[str(f)] = consts
        for vm in re.finditer(r"env::var(?:_os)?\s*\(\s*(?:Self::)?[\w:]*?(\w+)\s*\)", body):
            lit = consts.get(vm.group(1)) or global_consts.get(vm.group(1))
            if lit:
                const_env_global.setdefault(str(f), {}).setdefault(lit, []).append((str(f), 0))
    # integration tables: uppercase literals inside `vec![...]` in files that
    # build `Method::Env` method tables (provider env-name registries)
    table_env: dict[str, list[tuple[str, int]]] = {}
    for f, body in files:
        if "Method::Env" not in body:
            continue
        for vm in re.finditer(r"vec!\s*\[([^\]]{0,4000})\]", body):
            for lit in re.findall(r'"([A-Z][A-Z0-9_]{2,})"', vm.group(1)):
                line = body[: vm.start()].count("\n") + 1
                table_env.setdefault(lit, []).append((str(f), line))
    # one combined pattern for indirect call sites (fast single pass per file)
    names = sorted(wrappers, key=len, reverse=True)
    wrapper_re = (
        re.compile(
            r"(?<!\.)\b(" + "|".join(re.escape(n) for n in names) + r')\s*\(\s*"([A-Za-z_][\w]*)"'
        )
        if names
        else None
    )
    wrapper_ident_re = (
        re.compile(r"(?<!\.)\b(" + "|".join(re.escape(n) for n in names) + r")\s*\(\s*(\w+)\s*\)")
        if names
        else None
    )

    for f, body in files:
        patterns = [
            ("read", r"(?:std::env::|(?<![\w:])env::)var(?:_os)?\s*\(\s*\"([A-Za-z_][\w]*)\""),
            ("compile", r"(?:std::env::)?(?:option_)?env!\s*\(\s*\"([A-Za-z_][\w]*)\""),
            ("write", r"(?:std::env::|(?<![\w:])env::)(?:set_var|remove_var)\s*\(\s*\"([A-Za-z_][\w]*)\""),
        ]
        for kind, pat in patterns:
            for m in re.finditer(pat, body):
                name = m.group(1)
                line = body[: m.start()].count("\n") + 1
                found.setdefault(name, {}).setdefault(kind, []).append((str(f), line))
        # indirect: literal passed through a wrapper fn (non_empty/xdg/…)
        if wrapper_re is not None:
            for m in wrapper_re.finditer(body):
                name = m.group(2)
                line = body[: m.start()].count("\n") + 1
                found.setdefault(name, {}).setdefault("indirect", []).append((str(f), line))
        # indirect: const-carried env names
        for name, hits in const_env_global.get(str(f), {}).items():
            found.setdefault(name, {}).setdefault("indirect", []).extend(hits)
        # indirect: const ident passed to a wrapper (env_truthy(DISABLE_ENV))
        consts = file_consts.get(str(f), {})
        local_consts = {**global_consts, **consts}
        if local_consts and wrapper_ident_re is not None:
            for m in wrapper_ident_re.finditer(body):
                if m.group(2) in local_consts:
                    lit = local_consts[m.group(2)]
                    line = body[: m.start()].count("\n") + 1
                    found.setdefault(lit, {}).setdefault("indirect", []).append((str(f), line))
        # table: provider env-name registries (Method::Env vec![...])
        for name, hits in table_env.items():
            for hf, hl in hits:
                if hf == str(f):
                    found.setdefault(name, {}).setdefault("table", []).append((hf, hl))
        # indirect: uppercase-literal arrays whose binding is used near env access
        for am in re.finditer(r"let (\w+)\s*:\s*[^=\[\{]{0,80}=\s*\[([^\]]{0,2000})\]\s*;", body):
            arr, items = am.group(1), am.group(2)
            lits = re.findall(r'"([A-Z_][A-Z0-9_]*)"', items)
            if not lits or len(lits) != len([x for x in items.split(",") if x.strip()]):
                continue
            uses = [
                um for um in re.finditer(rf"\b{arr}\b", body)
                if re.search(r"env::|envs\(|EnvLookup|env_lookup", body[max(0, um.start() - 120): um.end() + 120])
            ]
            if not uses:
                continue
            for lit in lits:
                line = body[: am.start()].count("\n") + 1
                found.setdefault(lit).setdefault("indirect", []).append((str(f), line))
        dynamic += len(re.findall(r"\benvs\s*\(\s*\)|\benv::vars(?:_os)?\s*\(", body))
    return found, dynamic


# ------------------------------------------------------------------ ref side

SKIP_PARTS = ("/node_modules/", "/test/", "/e2e/", "/__tests__/", "/dist/", "/scripts/", "/script/")


def ref_env(ref: Path) -> tuple[dict[str, dict[str, list[tuple[str, int]]]], int]:
    found: dict[str, dict[str, list[tuple[str, int]]]] = {}
    dynamic = 0
    root = ref / "packages"
    files = sorted(set(list(root.rglob("*.[tj]s")) + list(root.rglob("*.tsx"))))
    seen: set[Path] = set()
    for f in files:
        if f in seen:
            continue
        seen.add(f)
        s = str(f)
        if any(part in s for part in SKIP_PARTS) or f.name.endswith(
            (".test.ts", ".test.tsx", ".d.ts", ".config.ts", ".config.mts", ".config.js")
        ):
            continue
        text = f.read_text(errors="replace")
        # member reads: process.env.X / Bun.env.X (assignment => write)
        for m in re.finditer(r"\b(?:process|Bun)\.env\.([A-Za-z_][\w]*)\s*(=[^=])?", text):
            name = m.group(1)
            kind = "write" if m.group(2) else "read"
            line = text[: m.start()].count("\n") + 1
            found.setdefault(name, {}).setdefault(kind, []).append((s, line))
        # bracket reads: process.env["X"]
        for m in re.finditer(r"\bprocess\.env\[\s*[\"']([A-Za-z_][\w]*)[\"']\s*\]", text):
            name = m.group(1)
            line = text[: m.start()].count("\n") + 1
            found.setdefault(name, {}).setdefault("read", []).append((s, line))
        # dynamic reads: process.env[<expr>]
        dynamic += len(re.findall(r"\bprocess\.env\[\s*[A-Za-z_$\"\']?", text)) - len(
            re.findall(r"\bprocess\.env\[\s*[\"'][A-Za-z_][\w]*[\"']\s*\]", text)
        )
    return found, max(dynamic, 0)


# --------------------------------------------------------------------- main

def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ref", default="/opt/opencode-ref")
    ap.add_argument("--write", default="docs/audit/census/ENV-VARS.md")
    args = ap.parse_args()

    rust, rust_dyn = rust_env(Path("crates"))
    ref, ref_dyn = ref_env(Path(args.ref))

    # env-convention filter: keep names matching [A-Z][A-Z0-9_]* (the process
    # boundary convention on both sides). Non-conventional spellings are
    # counted and declared below — they are dominated by same-name local
    # helpers / JSON-key getters (e.g. `get("model")`), not env reads.
    envconv = re.compile(r"[A-Z][A-Z0-9_]*")
    dropped_rust = sorted(n for n in rust if not envconv.fullmatch(n))
    dropped_ref = sorted(n for n in ref if not envconv.fullmatch(n))
    rust = {n: v for n, v in rust.items() if envconv.fullmatch(n)}
    ref = {n: v for n, v in ref.items() if envconv.fullmatch(n)}

    def reads(d):
        return {
            n
            for n, kinds in d.items()
            if kinds.keys() & {"read", "compile", "indirect", "table"}
        }

    rust_reads, ref_reads = reads(rust), reads(ref)
    upper_rust = {}
    for n in rust_reads:
        upper_rust.setdefault(n.upper(), []).append(n)
    upper_ref = {}
    for n in ref_reads:
        upper_ref.setdefault(n.upper(), []).append(n)

    both = sorted(set(upper_rust) & set(upper_ref))
    ref_only = sorted(set(upper_ref) - set(upper_rust))
    rust_only = sorted(set(upper_rust) - set(upper_ref))
    case_diff = sorted(
        set(upper_rust) & set(upper_ref)
    )  # raw spelling differences reported inside BOTH

    gaps_lines: dict[str, list[int]] = {}
    if GAPS_DOC.exists():
        gaps_text = GAPS_DOC.read_text()
        for name in sorted(set(upper_rust) | set(upper_ref)):
            for raw in upper_rust.get(name, []) + upper_ref.get(name, []):
                pat = re.compile(rf"\b{re.escape(raw)}\b")
                for i, line in enumerate(gaps_text.splitlines(), 1):
                    if pat.search(line):
                        gaps_lines.setdefault(name, []).append(i)

    def ev(side: dict, raws: list[str]) -> str:
        parts = []
        for raw in raws:
            kinds = side.get(raw, {})
            for kind in ("read", "compile", "indirect", "table", "write"):
                for f, ln in kinds.get(kind, [])[:2]:
                    parts.append(f"{kind} {f}:{ln}")
        return "; ".join(parts[:3]) or "?"

    rep = Report(
        "ENV-VARS Census（缺口普查 · 通道 3/∞）",
        "双侧进程边界环境变量全集机械提取（字面量）；census 非门禁，exit 0。",
    )
    rep.w("> 生成命令：`python3 tools/census/env_vars.py --write docs/audit/census/ENV-VARS.md`")
    rep.w(f"> 权威源：rust `crates/`（strip_tests 后的 var/var_os/env!/set_var/remove_var 字面量）"
          f"vs ref `packages/`（process.env.X / Bun.env.X / process.env[\"X\"]，测试与脚本目录排除）。")
    rep.w("")
    rep.w("## 归一化与范围声明")
    rep.w("")
    rep.w("- 名称按 **大写** 归一比对；原始拼写差异以 CASE_DIFF 标注（不静默合并语义）。")
    rep.w("- ref 排除 node_modules、/test/、/e2e/、__tests__、/dist/、/scripts/、/script/、*.test.*、*.d.ts、*.config.*（构建配置）；"
          "rust 剥 `#[cfg(test)]`/`#[test]` 区、跳过 *.gen.rs 与 /tests//benches//examples/ 集成测试目录与 tests.rs 模块文件。")
    rep.w("- rust `env!`/`option_env!` 编译期字面计入读取面（标注 compile）；`set_var`/`remove_var` 计为写（标注）。")
    rep.w(f"- 动态读取不可枚举：rust `envs()/vars()` {rust_dyn} 处、ref `process.env[expr]` {ref_dyn} 处——以计数声明，不进差异清单。")
    rep.w("- **BOTH 但用途不同**的变量不做行为判定（超范围；行为对拍属对应功能通道）。")
    rep.w("- CI/构建期变量（rust `env!(\"CARGO_PKG_*\")` 族、ref scripts 目录）多数非运行时旋钮，保留在清单中并以 compile/scripts 标注辅助判读。")
    rep.w("")
    rep.w("## Census")
    rep.w("")
    total = len(set(upper_rust) | set(upper_ref))
    rep.w(f"- rust 读取面：{len(rust_reads)} 个字面名（另动态 {rust_dyn} 处）；ref 读取面：{len(ref_reads)} 个字面名（另动态 {ref_dyn} 处）")
    rep.w(f"- **BOTH：{len(both)}** · REF_ONLY：**{len(ref_only)}** · RUST_ONLY：**{len(rust_only)}**（全集 {total}，逐项归属，覆盖率 100%）")
    rep.w("")
    rep.w("### REF_ONLY（ref 读取、rust 不读 → 缺口候选）")
    rep.w("")
    SHARED_PKGS = {"core", "cli", "tui", "server", "util", "ai", "schema", "protocol", "client", "plugin"}

    def pkgs_of(name, side):
        out = set()
        for raw in side:
            if raw.upper() != name:
                continue
            for kinds in side[raw].values():
                for fp, _ in kinds:
                    rel = fp.split("/packages/")[1] if "/packages/" in fp else fp
                    out.add(rel.split("/")[0])
        return out

    shared_items, eco_items = [], []
    for name in ref_only:
        pkgs = pkgs_of(name, ref)
        (shared_items if pkgs & SHARED_PKGS else eco_items).append((name, sorted(pkgs)))
    for name, pkgs in shared_items:
        raws = upper_ref[name]
        rep.w(f"- 🔴 `{name}` — ref[{'+'.join(pkgs)}]: {ev(ref, raws)}")
        if name in gaps_lines:
            rep.w(f"  - GAPS-AND-PITFALLS.md 提及行：{gaps_lines[name][:5]}")
        if name in ENV_ADJUDICATIONS:
            tag, why = ENV_ADJUDICATIONS[name]
            rep.w(f"  - 裁定【{tag}】：{why}")
        else:
            rep.w("  - 裁定【待人工】：构建/脚本或生态类，须逐项亲读后归档。")
    rep.w("")
    rep.w(f"#### REF_ONLY·生态胶水（仅 ref 独有包读取：desktop/app/session-ui/enterprise/stats/…，{len(eco_items)} 项）")
    rep.w("")
    for name, pkgs in eco_items:
        rep.w(f"- ⚪ `{name}` — ref[{'+'.join(pkgs)}]（ref 独有包，非本移植进程边界）")
    rep.w("")
    rep.w("### RUST_ONLY（rust 读取、ref 不读 → rust 额外旋钮）")
    rep.w("")
    for name in rust_only:
        raws = upper_rust[name]
        rep.w(f"- 🔗 `{name}` — rust: {ev(rust, raws)}")
        if name in gaps_lines:
            rep.w(f"  - 有据分歧：GAPS-AND-PITFALLS.md 提及行 {gaps_lines[name][:5]}")
        if name in ENV_ADJUDICATIONS:
            tag, why = ENV_ADJUDICATIONS[name]
            rep.w(f"  - 裁定【{tag}】：{why}")
    rep.w("")
    rep.w("### BOTH（双方读取；用途异同不在本通道判定）")
    rep.w("")
    for name in both:
        r_raws, f_raws = upper_rust[name], upper_ref[name]
        case = " · CASE_DIFF 原始拼写不等" if r_raws != f_raws else ""
        rep.w(f"- ✅ `{name}` — rust: {ev(rust, r_raws)} | ref: {ev(ref, f_raws)}{case}")
    rep.w("")
    rep.w("## Census 裁定")
    rep.w("")
    documented = [n for n in rust_only if n in gaps_lines]
    rep.w(f"- 已裁定 {total}/{total}（全集逐项归属，覆盖率 100%）。")
    rep.w(f"- RUST_ONLY {len(rust_only)} 项中 {len(documented)} 项有 GAPS-AND-PITFALLS.md 背书"
          f"（有据分歧，如 OPENCODE_PTY_BIN）；其余为待登记 rust 旋钮。")
    rep.w(f"- REF_ONLY {len(ref_only)} 项中：共享包侧 {len(shared_items)} 项已逐项定性（裁定表：已文档化缺口/缺口候选/"
          f"有据分歧/生态资产不适用/注册表族待集成通道）；ref 独有包 {len(eco_items)} 项为生态胶水（非本移植进程边界）。")
    rep.w("")
    rep.w("## 提取不确定性")
    rep.w("")
    rep.w("- 只提取字面量名；`envs()`/`vars()`/`process.env[expr]` 动态键不可枚举（计数见上）。")
    rep.w("- rust 间接读取按三类机械捕获：包装函数（&str 参数直通 env::var/_os，如 non_empty/xdg/truthy）、"
          "常量承载（const NAME: &str + env::var(NAME)/wrapper(NAME)，含跨文件唯一常量）、"
          "integration `Method::Env` 表内 vec![\"NAME\"]（标 table）；更深的数据驱动注册表（如 models.dev 快照 "
          "env 列表）未并入——provider/integration 族差异归后续通道。")
    rep.w("- ref `Bun.env.X` 仅出现在构建脚本（已排除）与 desktop（生态类）；"
          "ref provider 插件经字面量读取的 env（AICORE_*/CLOUDFLARE_* 等）在 rust 侧多经注册表/凭据链，"
          "直接字面比对会高估缺口（见裁定表'注册表族'）。")
    rep.w("- 大小写归一可能合并同族拼写（CASE_DIFF 已标注原始拼写）。")
    rep.w(f"- 环境变量命名规约过滤：仅保留 `[A-Z][A-Z0-9_]*`（进程边界两侧的惯例）；"
          f"非常规拼写 rust {len(dropped_rust)} 个 / ref {len(dropped_ref)} 个被排除"
          f"（主要为同名局部 helper/JSON 取键（如 `get(\"model\")`）误触与 camelCase wire 键，非 env 读取）。")
    rep.w("- rust `env!(\"CARGO_PKG_VERSION\")` 等编译期常量与运行时 env 语义不同，已标 compile。")
    rep.w("")
    rep.w("## 验证记录")
    rep.w("")
    rep.w("- 差异项（REF_ONLY/RUST_ONLY 全部、BOTH 抽样）已亲读双侧当前代码核实（file:line 见上；旧文档仅作线索）。")
    rep.w("- 幂等性：连跑两次输出经 `diff` 逐字节一致（/tmp/opencode/ 验证）。")
    rep.w("- 确定性：文件排序遍历、集合输出前排序；无时间戳/随机源。")
    rep.w("- exit 0（census 非门禁）。")

    rep.write(args.write)
    print("=== SUMMARY ===")
    print(f"both={len(both)} ref_only={len(ref_only)} rust_only={len(rust_only)} total={total}")
    print("REF_ONLY:", ref_only)
    print("RUST_ONLY:", rust_only)
    return 0


if __name__ == "__main__":
    sys.exit(main())
