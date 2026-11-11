# MIGRATIONS Census（数据库迁移 ID 集 · 通道 3）

> rust `MIGRATIONS` 静态数组 vs ref `migration/*.ts` 文件名 ID 集 + 生成清单交叉核对。

> 生成命令：`python3 tools/census/migrations.py --write docs/audit/census/MIGRATIONS.md`

## 归一化与范围声明

- rust 侧：`crates/oc-core/src/database/migrations.rs` 中 `MIGRATIONS: &[Migration]` 数组的 `id: "…"` 字面量（声明顺序即应用顺序）。
- ref 侧：`packages/core/src/database/migration/*.ts` 每文件一个迁移，drizzle 文件名 stem 即迁移 ID（时间戳前缀字典序即应用顺序）。
- ID 为逐字符串比较，不做任何名称变换。
- 范围外（明确排除）：v1→v2 转换器（rust `database/v1_migration.rs`、`v2_migration.rs`；ref `v1-migration*.ts`）、drizzle journal/meta 记账（rust 端为 journal 重播种，见 migrations.rs:12）。

## 轴 1/2：双侧 ID 集 diff

- rust（crates/oc-core/src/database/migrations.rs）：48 个迁移 ID
- ref（/opt/opencode-ref/packages/core/src/database/migration）：48 个迁移 ID
- **BOTH：48** · REF_ONLY：**0** · RUST_ONLY：**0**

## 顺序核对

- rust 声明顺序 == ref 文件名时间戳顺序：**一致**

## 轴 3：ref 生成清单交叉核对（migration.gen.ts）

- 清单导入 48 个 ID；migration 目录有而清单缺：**0**；清单有而目录缺：**0**

## 锚核对

- 登记册锚「48 迁移 ID 全同」：**成立（当前代码重取证一致）**

## Census 裁定

- 已裁定迁移 ID：48/48（覆盖率 100%：48/48 双侧全集逐项归属）
- 结论：双侧迁移 ID 集与顺序完全一致；REF_ONLY/RUST_ONLY > 0 时为**新缺口候选**。

## 验证记录

- 双侧每差异项均亲读当前代码核实（rust migrations.rs 的 id 字面量、ref migration 目录文件名、migration.gen.ts 导入清单）。
- 幂等性：脚本纯读取、无随机性；连跑两次输出逐字节一致（/tmp/opencode/ 下 diff 验证）。
- 旧文档仅作线索（锚），现状优先。

