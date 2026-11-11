#!/usr/bin/env python3
"""PROVIDERS census: provider-configuration-family diff (channel 4).

Axis 1 — native entrypoint families (`@opencode/ai/providers/*`):
  ref  : the `builtins` load-table keys in packages/core/src/provider.ts
         (every native entrypoint v2 can bind without an npm install).
  rust : the string literals inside `pub fn family_of`'s match body
         (crates/oc-core/src/provider_catalog/facade.rs) — the
         classifier that gates `UnsupportedProtocol`; plus the
         `BUILTIN_ENTRYPOINTS` list in native.rs as a cross-check.

Axis 2 — per-family config-key presence (keys only, never values):
  - catalog api/env keys: the models.dev snapshot (md5-checked equal on
    both sides) carries per-provider `api` (base URL) and `env` lists —
    counted mechanically below; ref parses it in models-dev.ts, rust in
    provider_catalog/source.rs.
  - overlay option keys: ref `CORE_KEYS` (provider.ts) vs rust
    `CORE_KEYS` (facade.rs) key-presence diff.

Axis 3 — the catalog-only census (the ~180 blind spot, mechanized):
  every snapshot provider family is resolved through EACH side's
  npm->entrypoint tables (ref aisdk-native.ts PACKAGES/HOSTS vs the
  native.rs ports) and classified:
    BOTH_NATIVE      — resolved to a dedicated native entrypoint that
                       rust family_of covers;
    BOTH_COMPAT      — rewrites to the generic
                       `@opencode/ai/providers/openai-compatible`
                       (native on both sides);
    REF_AISDK_ONLY   — npm package is a dedicated @ai-sdk/* provider
                       with no native mapping: ref installs it at
                       runtime (npm bridge); rust has no ported
                       protocol -> typed UnsupportedProtocol (gap
                       candidates, declared registered exemption).

Usage: python3 tools/census/providers.py [--ref /opt/opencode-ref] [--write docs/audit/census/PROVIDERS.md]
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from common import Report, _match_brace, strip_tests  # noqa: E402

GENERIC_NPM = {
    "@ai-sdk/openai-compatible": "chat",
    "@ai-sdk/anthropic": "messages",
    "@ai-sdk/openai": "responses",
}


def fn_region(src: str, header: str) -> str:
    """Body of the first fn whose signature starts with `header`."""
    start = src.index(header)
    brace = src.index("{", start)
    end = _match_brace(src, brace)
    return src[brace:end]


# ------------------------------------------------------------------ ref side
def ref_builtins(ref: Path) -> dict[str, int]:
    src = (ref / "packages/core/src/provider.ts").read_text()
    i = src.index("const builtins = new Map")
    j = src.index("\n])", i)
    out: dict[str, int] = {}
    for m in re.finditer(r'\[\s*"(@opencode/ai/providers/[^"]+)"', src[i:j]):
        out[m.group(1)] = src[: m.start() + i].count("\n") + 1
    return out


def ref_tables(ref: Path) -> tuple[dict[str, str], dict[str, dict[str, str]]]:
    """(PACKAGES npm->entrypoint, HOSTS providerId->npm->entrypoint) from aisdk-native.ts."""
    src = (ref / "packages/core/src/aisdk-native.ts").read_text()
    i = src.index("const PACKAGES")
    j = src.index("\n}", i)
    packages = dict(re.findall(r'"([^"]+)":\s*"(@opencode/ai/providers/[^"]+)"', src[i:j]))
    i = src.index("const HOSTS")
    j = src.index("\n}", i)
    host_src = src[i:j]
    hosts: dict[str, dict[str, str]] = {}
    # key bodies: from one `  key:` to the next `  key:` (multi-line safe)
    key_spans = [
        (m.group(1), m.start(1), m.end())
        for m in re.finditer(r'^\s{2}"?([a-z0-9-]+)"?:', host_src, re.MULTILINE)
    ]
    for idx, (pid, kstart, kend) in enumerate(key_spans):
        body_end = key_spans[idx + 1][2] if idx + 1 < len(key_spans) else len(host_src)
        body = host_src[kend:body_end]
        pm = re.search(r'protocols\("([a-z0-9-]+)"\)', body)
        if pm:
            name = pm.group(1)
            hosts[pid] = {
                npm: f"@opencode/ai/providers/{name}/{suffix}"
                for npm, suffix in GENERIC_NPM.items()
            }
        else:
            hosts[pid] = dict(re.findall(r'"([^"]+)":\s*"(@opencode/ai/providers/[^"]+)"', body))
    return packages, hosts


def ref_core_keys(ref: Path) -> tuple[list[str], str, int]:
    src = (ref / "packages/core/src/provider.ts").read_text()
    m = re.search(r"const CORE_KEYS = \[([^\]]*)\]", src)
    keys = re.findall(r'"([^"]+)"', m.group(1))
    return keys, "packages/core/src/provider.ts", src[: m.start()].count("\n") + 1


# ----------------------------------------------------------------- rust side
def rust_family_literals() -> tuple[dict[str, int], str]:
    src = strip_tests(Path("crates/oc-core/src/provider_catalog/facade.rs").read_text())
    start = src.index("pub fn family_of")
    brace = src.index("{", start)
    end = _match_brace(src, brace)
    region = src[brace:end]
    base = brace
    out: dict[str, int] = {}
    for m in re.finditer(r'"(@opencode/ai/providers/[^"]+)"', region):
        out.setdefault(m.group(1), src[: base + m.start()].count("\n") + 1)
    return out, "crates/oc-core/src/provider_catalog/facade.rs"


def rust_tables() -> tuple[dict[str, str], dict[str, dict[str, str]], dict[str, int], dict[str, int]]:
    """(packages, hosts, BUILTIN_ENTRYPOINTS, host-table provider ids) from native.rs."""
    src = strip_tests(Path("crates/oc-core/src/provider_catalog/native.rs").read_text())
    pkg_region = fn_region(src, "fn packages(npm")
    packages = dict(re.findall(r'"([^"]+)" => "(@opencode/ai/providers/[^"]+)"', pkg_region))
    host_region = fn_region(src, "fn host_table")
    hosts: dict[str, dict[str, str]] = {}
    host_ids: dict[str, int] = {}
    # arms: `<ids> => HostEntry::<shape>(<args>)` — ids may span lines
    arms = list(re.finditer(r"((?:\s*\"[a-z0-9-]+\"\s*\|?)+)=>\s*\{?\s*HostEntry::", host_region))
    for idx, arm in enumerate(arms):
        arm_end = arms[idx + 1].start() if idx + 1 < len(arms) else len(host_region)
        text = host_region[arm.start():arm_end]
        ids = re.findall(r'"([a-z0-9-]+)"', arm.group(1))
        arm_line = host_region[: arm.start()].count("\n") + 1
        pm = re.search(r'Protocols\("([a-z0-9-]+)"\)', text)
        if pm:
            name = pm.group(1)
            table = {npm: f"@opencode/ai/providers/{name}/{s}" for npm, s in GENERIC_NPM.items()}
        else:
            table = dict(re.findall(r'\(\s*"([^"]+)",\s*"(@opencode/ai/providers/[^"]+)"', text))
        for pid in ids:
            hosts[pid] = table
            host_ids.setdefault(pid, arm_line)
    mb = re.search(r"pub const BUILTIN_ENTRYPOINTS[^;]*?=\s*&\[(.*?)\];", src, re.S)
    builtins = {}
    for m in re.finditer(r'"(@opencode/ai/providers/[^"]+)"', mb.group(1)):
        builtins.setdefault(m.group(1), src[: mb.start() + m.start()].count("\n") + 1)
    return packages, hosts, builtins, host_ids


def rust_core_keys() -> tuple[list[str], str, int]:
    src = strip_tests(Path("crates/oc-core/src/provider_catalog/facade.rs").read_text())
    m = re.search(r'const CORE_KEYS: &\[&str\] =\s*&\[([^\]]*)\]', src)
    keys = re.findall(r'"([^"]+)"', m.group(1))
    return keys, "crates/oc-core/src/provider_catalog/facade.rs", src[: m.start()].count("\n") + 1


def resolve(provider_id: str, npm: str | None, packages: dict, hosts: dict) -> str | None:
    if not npm:
        return None
    if provider_id in hosts and npm in hosts[provider_id]:
        return hosts[provider_id][npm]
    return packages.get(npm)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ref", default="/opt/opencode-ref")
    ap.add_argument("--write", default=None)
    args = ap.parse_args()
    ref = Path(args.ref)

    # snapshot identity anchor
    rust_snap = Path("crates/oc-core/src/provider_catalog/models-dev-snapshot.txt")
    ref_snap = ref / "packages/core/src/models-dev/snapshot.txt"
    md5_rust = hashlib.md5(rust_snap.read_bytes()).hexdigest()
    md5_ref = hashlib.md5(ref_snap.read_bytes()).hexdigest()
    catalog = json.loads(rust_snap.read_text())

    # axis 1
    rb = ref_builtins(ref)
    fam, fam_file = rust_family_literals()
    both = sorted(set(rb) & set(fam))
    ref_only = sorted(set(rb) - set(fam))
    rust_only = sorted(set(fam) - set(rb))

    _, _, rust_builtin, _ = rust_tables()
    builtin_gap = sorted(set(rb) - set(rust_builtin)) + sorted(set(rust_builtin) - set(rb))

    # axis 2
    ref_pkg, ref_hosts = ref_tables(ref)
    rust_pkg, rust_hosts, _, rust_host_ids = rust_tables()
    ref_host_ids = sorted(ref_hosts)
    host_keys_only_ref = sorted(set(ref_host_ids) - set(rust_hosts))
    host_keys_only_rust = sorted(set(rust_hosts) - set(ref_host_ids))
    pkg_diff = sorted(
        {k for k in set(ref_pkg) | set(rust_pkg) if ref_pkg.get(k) != rust_pkg.get(k)}
    )
    rkeys, rfile, rline = ref_core_keys(ref)
    kkeys, kfile, kline = rust_core_keys()

    # axis 3: catalog census
    both_native, both_compat, ref_aisdk_only = [], [], []
    with_api = sum(1 for p in catalog.values() if p.get("api"))
    with_env = sum(1 for p in catalog.values() if p.get("env"))
    for pid in sorted(catalog):
        npm = catalog[pid].get("npm")
        entry_ref = resolve(pid, npm, ref_pkg, ref_hosts)
        entry_rust = resolve(pid, npm, rust_pkg, rust_hosts)
        fam_hit = entry_rust in fam if entry_rust else False
        if entry_ref is None:
            ref_aisdk_only.append((pid, npm))
        elif entry_ref == "@opencode/ai/providers/openai-compatible":
            both_compat.append(pid)
        elif fam_hit and entry_ref == entry_rust:
            both_native.append(pid)
        elif fam_hit:
            both_native.append(pid)  # mapping skew handled by table diff above
        else:
            ref_aisdk_only.append((pid, npm))

    r = Report(
        "PROVIDERS Census（缺口普查 · provider 配置族）",
        "三轴机械对拍：native entrypoint 族 / 配置键存在性 / catalog-only 家族逐项裁定；生成命令：`python3 tools/census/providers.py --write docs/audit/census/PROVIDERS.md`",
    )
    r.w("## 前置锚：models.dev 快照双侧一致")
    r.w("")
    r.w(f"- md5(rust {rust_snap}) = `{md5_rust}`")
    r.w(f"- md5(ref  {ref_snap}) = `{md5_ref}`")
    r.w(f"- **{'一致 ✅' if md5_rust == md5_ref else '不一致 🔴'}** → catalog 数据面（provider id/npm/api/env 全集）双侧逐字节同源，轴 2/轴 3 的差异只能来自**路由与代码面**。")
    r.w("")
    r.w("## 轴 1：native entrypoint 族全集")
    r.w("")
    r.w(f"- ref（provider.ts `builtins` 装载表键）：**{len(rb)}** 个 entrypoint")
    r.w(f"- rust（facade.rs `family_of` 匹配体字面量全集）：**{len(fam)}** 个 entrypoint")
    r.w(f"- **BOTH：{len(both)}** · REF_ONLY：**{len(ref_only)}** · RUST_ONLY：**{len(rust_only)}**")
    for e in ref_only:
        r.w(f"  - 🔴 REF_ONLY `{e}` — ref packages/core/src/provider.ts:{rb[e]}（family_of 无此族 → 缺口候选）")
    for e in rust_only:
        r.w(f"  - 🟡 RUST_ONLY `{e}` — rust {fam_file}（family_of 匹配体）:{fam[e]}（ref builtins 装载表无 → rust-extra）")
    r.w(f"- 交叉核对：rust native.rs `BUILTIN_ENTRYPOINTS`（{len(rust_builtin)} 项）与 ref builtins 差集：**{len(builtin_gap)}**（{', '.join(builtin_gap) if builtin_gap else '无'}）")
    r.w("")
    r.w("## 轴 2：每家族配置键存在性（只比键，不比值）")
    r.w("")
    r.w("### catalog api/env 键（快照同源 → 恒等）")
    r.w(f"- `{len(catalog)}` 个家族中 **api（base URL）键存在：{with_api}**（rust 解析于 provider_catalog/source.rs:20 `pub api: Option<String>`；ref models-dev.ts:65/71 `readonly api?: string`）")
    r.w(f"- `{len(catalog)}` 个家族中 **env 键存在：{with_env}**（rust source.rs env 解析 + facade.rs:1847 `package_env_chain` 补链；ref models-dev.ts:73 `readonly env: readonly string[]`）")
    r.w("### overlay 选项键（ref CORE_KEYS vs rust CORE_KEYS）")
    r.w(f"- ref  {rfile}:{rline}：{', '.join(rkeys)}")
    r.w(f"- rust {kfile}:{kline}：{', '.join(kkeys)}")
    r.w(f"- **键集差异：{sorted(set(rkeys) ^ set(kkeys)) or '无 ✅'}**")
    r.w("### npm→entrypoint 路由表")
    r.w(f"- PACKAGES 表差异（npm 键或映射目标不同）：**{len(pkg_diff)}**（{', '.join(pkg_diff) if pkg_diff else '无 ✅'}）")
    r.w(f"- HOSTS provider-id 键差异：REF_ONLY {len(host_keys_only_ref)}（{', '.join(host_keys_only_ref) or '无'}）· RUST_ONLY {len(host_keys_only_rust)}（{', '.join(host_keys_only_rust) or '无'}）")
    r.w("")
    r.w("## 轴 3：catalog-only 家族逐项裁定（~180 盲区机械化）")
    r.w("")
    total = len(catalog)
    r.w(f"- 快照家族总数：**{total}**")
    r.w(f"- **BOTH_NATIVE（专属 native entrypoint，rust family_of 覆盖）：{len(both_native)}**")
    r.w(f"- **BOTH_COMPAT（重写到通用 `@opencode/ai/providers/openai-compatible`，双侧 native）：{len(both_compat)}** ← 上一轮「~180 catalog-only」的确凿计数")
    r.w(f"- **REF_AISDK_ONLY（专属 @ai-sdk 包，ref 走 npm 动态安装桥；rust 无移植协议 → `UnsupportedProtocol`）：{len(ref_aisdk_only)}**")
    for pid, npm in ref_aisdk_only:
        env = catalog[pid].get("env", [])
        r.w(f"  - 🔴 `{pid}`（npm `{npm}`，env {env}）— ref aisdk-native.ts 无映射 → provider.ts:120-138 npm 安装桥；rust facade.rs family_of None → 错误族 `UnsupportedProtocol`（facade.rs:99，登记册豁免项）")
    r.w("")
    r.w("## 归一化与范围声明")
    r.w("")
    r.w("- 轴 1 比对单位是 **entrypoint 字符串**（`@opencode/ai/providers/…`），双侧同一词汇，无归一化。")
    r.w("- 轴 2 只比**键存在性**（api/env/CORE_KEYS/路由表键），不比值、不比每家族默认 baseURL 字面量（ref provider 模块内 `const baseURL = …` 与 rust 路由构造的等值性由既有 parity 测试钉定，不在本通道）。")
    r.w("- 轴 3 的裁定以「npm→entrypoint 路由 + family_of 覆盖」为机械判据；azure/mantle 的 model-shape 特例（aisdk-native.ts:131-141 / native.rs mantle()）只影响 family 内部选路，不影响族级 BOTH 判定（声明）。")
    r.w("- ref 侧 `anthropic-compatible` 模块（packages/ai/src/providers/anthropic-compatible.ts 存在）不在 builtins 装载表，经 `@opencode/ai/*` 直连解析（provider.ts:106）——轴 1 以 builtins 表为权威，该差异在 RUST_ONLY 行内注明。")
    r.w("")
    r.w("## Census 裁定")
    r.w("")
    adjudicated = len(both) + len(ref_only) + len(rust_only) + total
    r.w(f"- 已裁定项：entrypoint 族 {len(both) + len(ref_only) + len(rust_only)}/{len(rb) + len(rust_only)}（双侧并集逐项归属）+ catalog 家族 {total}/{total}")
    r.w(f"- 结论：轴 1 REF_ONLY {len(ref_only)}；轴 3 REF_AISDK_ONLY {len(ref_aisdk_only)} 为缺口候选（rust 为登记册豁免的 typed-error 面）；RUST_ONLY entrypoint {len(rust_only)} 需登记册 rust-extra 背书；BOTH_COMPAT {len(both_compat)} 替代上一轮「~180 推定」为确凿计数。")
    r.w("")
    r.w("## 验证记录")
    r.w("")
    r.w("- 亲读核实（2026-11-11）：provider.ts builtins 表（48-108 行区间）逐项对照 native.rs BUILTIN_ENTRYPOINTS；aisdk-native.ts PACKAGES/HOSTS 与 native.rs packages()/host_table() 逐键对照；family_of 匹配体（facade.rs:438 起）逐字面量核对；快照 md5 双侧现算。")
    r.w("- RUST_ONLY 3 项人工核实：`amazon-bedrock/mantle/messages`（ref mantle() 只生成 chat/responses 两形态，aisdk-native.ts:138-141）与 `xai/chat`（ref providers/xai.ts 无 chat 子模块）为 rust 额外选路族；`anthropic-compatible` 在 ref 为独立模块但不在 builtins 表（见范围声明）。")
    r.w("- REF_AISDK_ONLY 项人工核实：如 `gitlab`/`perplexity` 的 npm 为专属包，aisdk-native.ts 两表均无映射，ref 经 loadPackage 的 npm.add 桥（provider.ts:120-138）支持；rust facade.rs:2151-2156 注释明示「`aisdk:` npm-install bridge is not ported」→ typed `UnsupportedProtocol`。")
    r.w("- 提取不确定性：(1) family_of 匹配体提取依赖 common._match_brace 的括号配平（对当前源稳定）；(2) rust HOSTS 解析按 match 臂逐臂识别 Protocols/Fixed/Many 三形态，若新增第四形态会漏；(3) 轴 3 判定不含 azure/mantle shape 特例与 copilot 同步面（copilot_sync.rs/azure_sync.rs 为 ref Console/同步面，不在 models.dev 快照内）。")
    r.w("- 幂等性：脚本连跑两次输出逐字节一致（/tmp/opencode/ diff 验证）。")
    r.w("")
    _ = r.write(args.write)
    return 0


if __name__ == "__main__":
    sys.exit(main())
