#!/usr/bin/env python3
"""HTTP-surface census: dual-side runtime-self-report diff for the HTTP channel.

Axis 1 — authoritative specs:   ref packages/protocol/openapi.json (generated
                                 from the effect HttpApi definition) vs rust
                                 tools/openapi-gen/openapi.json (pinned source
                                 of routes.gen.rs, sha-checked in tests).
Axis 2 — rust production mounts: every axum `.route()` in oc-server src,
                                 test regions excluded, multi-line calls and
                                 MethodRouter chains parsed, path params
                                 normalized (`:x` / `*x` / `{x}` -> canonical).
Axis 3 — ref non-HttpApi paths:  string literals that look like served paths
                                 in packages/server/src, to catch anything
                                 mounted outside the openapi-generating Api.

Output: markdown census report (statuses: BOTH / REF_ONLY / RUST_ONLY /
RUST_EXTRA_MOUNT / REF_EXTRA_PATH), exit 0 always — this is a census, not a
gate. New RUST_EXTRA_MOUNT / REF_ONLY rows are gap candidates for the
register; RUST_ONLY vs ref are documented rust-extra divergences.

Usage: python3 tools/census/http_surface.py [--ref /opt/opencode-ref] [--write docs/audit/census/HTTP-SURFACE.md]
"""
from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

from common import VERBS, Report, canon_axum, canon_ref, strip_tests  # noqa: E402



# ---------------------------------------------------------------- axis 1
def spec_ops(path: Path) -> set[tuple[str, str, str]]:
    doc = json.loads(path.read_text())
    ops = set()
    for p, methods in doc.get("paths", {}).items():
        for m, op in methods.items():
            if m.lower() in VERBS:
                ops.add((m.upper(), canon_ref(p), op.get("operationId", "")))
    return ops


# ---------------------------------------------------------------- axis 2



def axum_routes(src_dir: Path) -> list[tuple[str, set[str], str, int]]:
    """(canonical path, verbs, file, line) for every production `.route(`."""
    results = []
    for f in sorted(src_dir.rglob("*.rs")):
        if f.name.endswith(".gen.rs"):
            continue
        raw = f.read_text()
        body = strip_tests(raw)
        for m in re.finditer(r"\.route\(", body):
            # path literal: first string after the paren (may be next line)
            tail = body[m.end():]
            lit = re.match(r"\s*\"([^\"]+)\"", tail)
            if not lit:
                continue
            path = lit.group(1)
            # argument region: to the balanced closing paren of .route(
            depth, i = 1, m.end() - 1  # position of '('
            j = m.end()
            while j < len(body) and depth > 0:
                if body[j] == "(":
                    depth += 1
                elif body[j] == ")":
                    depth -= 1
                j += 1
            region = body[m.end():j]
            verbs = {v.upper() for v in VERBS if re.search(rf"\b{v}\s*\(", region)}
            if not verbs:
                verbs = {"(handler)"}
            line = body[: m.start()].count("\n") + 1
            results.append((canon_axum(path), verbs, str(f), line))
            _ = i
    return results


# ---------------------------------------------------------------- axis 3
def ref_path_literals(server_src: Path, known: set[str]) -> list[tuple[str, str, int]]:
    """Path-looking string literals in ref server sources outside the spec."""
    hits = []
    for f in sorted(server_src.rglob("*.ts")):
        if "/test/" in str(f) or f.name.endswith(".test.ts"):
            continue
        for i, line in enumerate(f.read_text().splitlines(), 1):
            for lit in re.findall(r"[\"'](/[a-zA-Z0-9_:*-]+(?:/[a-zA-Z0-9_:{}*-]+)*)[\"']", line):
                if not (lit.startswith("/api/") or lit.startswith("/auth/")):
                    continue
                if canon_ref(lit) in known:
                    continue
                hits.append((lit, str(f), i))
    return hits


# ---------------------------------------------------------------- main
def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ref", default="/opt/opencode-ref")
    ap.add_argument("--write", default=None)
    args = ap.parse_args()

    ref_spec = Path(args.ref) / "packages/protocol/openapi.json"
    rust_spec = Path("tools/openapi-gen/openapi.json")

    ref_ops = spec_ops(ref_spec)
    rust_ops = spec_ops(rust_spec)
    ref_idx = {(m, p): op for m, p, op in ref_ops}
    rust_idx = {(m, p): op for m, p, op in rust_ops}

    both = sorted(set(ref_idx) & set(rust_idx))
    ref_only = sorted(set(ref_idx) - set(rust_idx))
    rust_only = sorted(set(rust_idx) - set(ref_idx))

    mounts = axum_routes(Path("crates/oc-server/src"))
    mount_idx: dict[tuple[str, str], tuple[str, int]] = {}
    for path, verbs, file, line in mounts:
        for v in verbs:
            mount_idx.setdefault((v, path), (file, line))
    rust_spec_paths = {(m, p) for m, p, _ in rust_ops}
    extra_mounts = sorted(
        (p, v, mount_idx[(v, p)][0], mount_idx[(v, p)][1])
        for (v, p) in mount_idx
        if (v, p) not in rust_spec_paths
    )
    unmounted = sorted(k for k in rust_spec_paths if k not in mount_idx)

    ref_known = {p for _, p, _ in ref_ops}
    ref_extra = ref_path_literals(Path(args.ref) / "packages/server/src", ref_known)

    lines = []
    w = lines.append
    w("# HTTP-Surface Census（最小验证 · 通道 1/∞）")
    w("")
    w("> 双侧权威自报对拍 + 生产挂载提取 + ref 非 spec 路径字面量三轴。")
    w("> 生成命令：`python3 tools/census/http_surface.py --write docs/audit/census/HTTP-SURFACE.md`")
    w("")
    w("## 轴 1：双侧 openapi 自报 diff")
    w("")
    w(f"- ref（{ref_spec}）：{len(ref_ops)} operations")
    w(f"- rust（{rust_spec}，sha 钉定于 app_shell/assets 测试）：{len(rust_ops)} operations")
    w(f"- **BOTH：{len(both)}** · REF_ONLY：**{len(ref_only)}** · RUST_ONLY：**{len(rust_only)}**")
    for m, p in ref_only:
        w(f"  - 🔴 REF_ONLY `{m} {p}`（ref 有、rust 无 → 缺口候选）")
    for m, p in rust_only:
        w(f"  - 🔗 RUST_ONLY `{m} {p}`（rust 额外面，需登记册决策背书）")
    w("")
    w("## 轴 2：rust 生产挂载面（oc-server `.route(`，测试区已剥除）")
    w("")
    w(f"- 提取 `.route(` 调用 {len(mounts)} 处（含多行调用与 MethodRouter 链），"
      f"唯一 method×path {len(mount_idx)} 个")
    w(f"- openapi 142 项中**未挂载：{len(unmounted)}**（>0 即与 `handlers_all_142_operations_mounted` 探针测试矛盾，需排查）")
    for m, p in unmounted:
        w(f"  - ⚠️ UNMOUNTED `{m} {p}`")
    w(f"- **spec 之外的生产挂载：{len(extra_mounts)}**（rust 服务了 openapi 未声明的面 → 缺口候选）")
    for p, v, f, ln in extra_mounts:
        w(f"  - 🟡 RUST_EXTRA_MOUNT `{v} {p}` — {f}:{ln}")
    w("")
    w("## 轴 3：ref 非 spec 路径字面量（packages/server/src，openapi 之外的候选）")
    w("")
    w(f"- **REF_EXTRA_PATH：{len(ref_extra)}**")
    for lit, f, ln in ref_extra:
        w(f"  - 🟡 `{lit}` — {f}:{ln}")
    w("")
    w("## Census 裁定")
    w("")
    adjudicated = len(both) + len(ref_only) + len(rust_only)
    w(f"- 已裁定 operation：{adjudicated}/{adjudicated}（覆盖率 100%：双侧 spec 全集逐项归属）")
    w(f"- 结论：BOTH {len(both)} 项由 `handlers_route_coverage.rs` 运行时探针钉定挂载；"
      f"RUST_ONLY {len(rust_only)} 项需登记册条目背书；REF_ONLY/RUST_EXTRA_MOUNT/REF_EXTRA_PATH > 0 时为**新缺口候选**。")
    report = "\n".join(lines) + "\n"

    if args.write:
        out = Path(args.write)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(report)
        print(f"wrote {out}")
    print(report)
    return 0


if __name__ == "__main__":
    sys.exit(main())
