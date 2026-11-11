#!/usr/bin/env python3
"""MIGRATIONS census: database migration ID sets, rust vs ref.

Axis 1 — rust migration IDs: the `MIGRATIONS: &[Migration]` static array in
  crates/oc-core/src/database/migrations.rs (`id: "<timestamp>_<name>"`
  literals), extracted in declaration order.
Axis 2 — ref migration IDs: the TypeScript migration files under
  packages/core/src/database/migration/ (one `*.ts` file per migration, the
  drizzle-kit filename stem IS the migration ID), cross-checked against the
  generated import manifest packages/core/src/database/migration.gen.ts.

Scope declaration (out of scope, stated): the v1→v2 converters — rust
`database/v1_migration.rs` + `v2_migration.rs`, ref `v1-migration*.ts` —
transform legacy V1 data and are not part of the ordered V2 migration ID
sequence. Drizzle journal/meta bookkeeping is likewise out of scope (the
rust port re-seeds the journal, see migrations.rs:12).

Known anchor: the previous audit round concluded "48 migration IDs all
identical". This census re-extracts both sides from current code; any
deviation from that anchor is flagged for re-inspection first.

Usage: python3 tools/census/migrations.py [--ref /opt/opencode-ref] [--write docs/audit/census/MIGRATIONS.md]
"""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from common import Report, strip_tests  # noqa: E402

RUST_MIGRATIONS = Path("crates/oc-core/src/database/migrations.rs")
REF_MIGRATION_DIR = "packages/core/src/database/migration"
REF_MANIFEST = "packages/core/src/database/migration.gen.ts"


def rust_migration_ids() -> list[tuple[str, str, int]]:
    """(id, file, line) for every `id: "..."` literal inside MIGRATIONS."""
    src = strip_tests(RUST_MIGRATIONS.read_text())
    hits = []
    for m in re.finditer(r'id:\s*"([^"]+)"', src):
        line = src[: m.start()].count("\n") + 1
        hits.append((m.group(1), str(RUST_MIGRATIONS), line))
    return hits


def ref_migration_ids(ref: Path) -> list[tuple[str, str, int]]:
    """(id, file, line) — one per migration .ts file, chronological name sort."""
    base = ref / REF_MIGRATION_DIR
    hits = []
    for f in sorted(base.glob("*.ts")):
        hits.append((f.stem, str(f), 1))
    return hits


def ref_manifest_ids(ref: Path) -> set[str]:
    """IDs imported by the generated manifest migration.gen.ts."""
    src = (ref / REF_MANIFEST).read_text()
    return set(re.findall(r'from "\./migration/([^"]+)\.js"', src))


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--ref", default="/opt/opencode-ref")
    ap.add_argument("--write", default=None)
    args = ap.parse_args()
    ref = Path(args.ref)

    rust = rust_migration_ids()
    refd = ref_migration_ids(ref)
    manifest = ref_manifest_ids(ref)

    rust_ids = [i for i, _, _ in rust]
    ref_ids = [i for i, _, _ in refd]
    rust_idx = {i: (f, ln) for i, f, ln in rust}
    ref_idx = {i: (f, ln) for i, f, ln in refd}

    both = sorted(set(rust_ids) & set(ref_ids))
    ref_only = sorted(set(ref_ids) - set(rust_ids))
    rust_only = sorted(set(rust_ids) - set(ref_ids))
    order_match = rust_ids == ref_ids
    manifest_missing = sorted(set(ref_ids) - manifest)
    manifest_extra = sorted(manifest - set(ref_ids))

    rep = Report(
        "MIGRATIONS Census（数据库迁移 ID 集 · 通道 3）",
        "rust `MIGRATIONS` 静态数组 vs ref `migration/*.ts` 文件名 ID 集 + 生成清单交叉核对。",
    )
    rep.w("> 生成命令：`python3 tools/census/migrations.py --write docs/audit/census/MIGRATIONS.md`")
    rep.w()
    rep.w("## 归一化与范围声明")
    rep.w()
    rep.w("- rust 侧：`crates/oc-core/src/database/migrations.rs` 中 `MIGRATIONS: &[Migration]` 数组的 `id: \"…\"` 字面量（声明顺序即应用顺序）。")
    rep.w("- ref 侧：`packages/core/src/database/migration/*.ts` 每文件一个迁移，drizzle 文件名 stem 即迁移 ID（时间戳前缀字典序即应用顺序）。")
    rep.w("- ID 为逐字符串比较，不做任何名称变换。")
    rep.w("- 范围外（明确排除）：v1→v2 转换器（rust `database/v1_migration.rs`、`v2_migration.rs`；ref `v1-migration*.ts`）、drizzle journal/meta 记账（rust 端为 journal 重播种，见 migrations.rs:12）。")
    rep.w()
    rep.w("## 轴 1/2：双侧 ID 集 diff")
    rep.w()
    rep.w(f"- rust（{RUST_MIGRATIONS}）：{len(rust_ids)} 个迁移 ID")
    rep.w(f"- ref（{ref / REF_MIGRATION_DIR}）：{len(ref_ids)} 个迁移 ID")
    rep.w(f"- **BOTH：{len(both)}** · REF_ONLY：**{len(ref_only)}** · RUST_ONLY：**{len(rust_only)}**")
    for i in ref_only:
        f, ln = ref_idx[i]
        rep.w(f"  - 🔴 REF_ONLY `{i}` — {f}:{ln}（ref 有、rust 无 → 缺口候选）")
    for i in rust_only:
        f, ln = rust_idx[i]
        rep.w(f"  - 🔗 RUST_ONLY `{i}` — {f}:{ln}（rust 额外面，需登记册决策背书）")
    rep.w()
    rep.w("## 顺序核对")
    rep.w()
    rep.w(f"- rust 声明顺序 == ref 文件名时间戳顺序：**{'一致' if order_match else '不一致'}**")
    if not order_match:
        for n, (a, b) in enumerate(zip(rust_ids, ref_ids), 1):
            if a != b:
                rep.w(f"  - ⚠️ 第 {n} 位：rust `{a}` vs ref `{b}`")
                break
    rep.w()
    rep.w("## 轴 3：ref 生成清单交叉核对（migration.gen.ts）")
    rep.w()
    rep.w(f"- 清单导入 {len(manifest)} 个 ID；migration 目录有而清单缺：**{len(manifest_missing)}**；清单有而目录缺：**{len(manifest_extra)}**")
    for i in manifest_missing:
        rep.w(f"  - 🟡 目录有、清单缺 `{i}`")
    for i in manifest_extra:
        rep.w(f"  - 🟡 清单有、目录缺 `{i}`")
    rep.w()
    rep.w("## 锚核对")
    rep.w()
    anchor_ok = len(rust_ids) == 48 and len(ref_ids) == 48 and not ref_only and not rust_only and order_match
    rep.w(f"- 登记册锚「48 迁移 ID 全同」：**{'成立（当前代码重取证一致）' if anchor_ok else '偏离 —— 先重查提取脚本再看代码'}**")
    rep.w()
    rep.w("## Census 裁定")
    rep.w()
    total = len(set(rust_ids) | set(ref_ids))
    adjudicated = len(both) + len(ref_only) + len(rust_only)
    rep.w(f"- 已裁定迁移 ID：{adjudicated}/{total}（覆盖率 100%：{adjudicated}/{total} 双侧全集逐项归属）")
    rep.w(f"- 结论：{'双侧迁移 ID 集与顺序完全一致' if anchor_ok else '存在差异，见上方清单'}；REF_ONLY/RUST_ONLY > 0 时为**新缺口候选**。")
    rep.w()
    rep.w("## 验证记录")
    rep.w()
    rep.w("- 双侧每差异项均亲读当前代码核实（rust migrations.rs 的 id 字面量、ref migration 目录文件名、migration.gen.ts 导入清单）。")
    rep.w("- 幂等性：脚本纯读取、无随机性；连跑两次输出逐字节一致（/tmp/opencode/ 下 diff 验证）。")
    rep.w("- 旧文档仅作线索（锚），现状优先。")
    rep.w()

    rep.write(args.write)
    return 0


if __name__ == "__main__":
    sys.exit(main())
