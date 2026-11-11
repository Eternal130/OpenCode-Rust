# HTTP-Surface Census（最小验证 · 通道 1/∞）

> 双侧权威自报对拍 + 生产挂载提取 + ref 非 spec 路径字面量三轴。
> 生成命令：`python3 tools/census/http_surface.py --write docs/audit/census/HTTP-SURFACE.md`

## 轴 1：双侧 openapi 自报 diff

- ref（/opt/opencode-ref/packages/protocol/openapi.json）：141 operations
- rust（tools/openapi-gen/openapi.json，sha 钉定于 app_shell/assets 测试）：142 operations
- **BOTH：141** · REF_ONLY：**0** · RUST_ONLY：**1**
  - 🔗 RUST_ONLY `POST /api/plugin/install`（rust 额外面，需登记册决策背书）

## 轴 2：rust 生产挂载面（oc-server `.route(`，测试区已剥除）

- 提取 `.route(` 调用 142 处（含多行调用与 MethodRouter 链），唯一 method×path 142 个
- openapi 142 项中**未挂载：0**（>0 即与 `handlers_all_142_operations_mounted` 探针测试矛盾，需排查）
- **spec 之外的生产挂载：0**（rust 服务了 openapi 未声明的面 → 缺口候选）

## 轴 3：ref 非 spec 路径字面量（packages/server/src，openapi 之外的候选）

- **REF_EXTRA_PATH：0**

## Census 裁定

- 已裁定 operation：142/142（覆盖率 100%：双侧 spec 全集逐项归属）
- 结论：BOTH 141 项由 `handlers_route_coverage.rs` 运行时探针钉定挂载；RUST_ONLY 1 项需登记册条目背书；REF_ONLY/RUST_EXTRA_MOUNT/REF_EXTRA_PATH > 0 时为**新缺口候选**。
## 验证记录（2026-11-11）

1. **运行时探针复跑 PASS**：`cargo nextest run -p oc-server --test handlers_route_coverage` → `handlers_all_142_operations_mounted` 1 passed（真实 boot + tower oneshot，未挂载路由落空 404 即失败）。
2. **RUST_ONLY 背书成立**：`POST /api/plugin/install` 已由登记册三处记载（G-17 rust-extra 家族：GAPS-AND-PITFALLS.md:487/:734/:1028；misc.rs:25 注释自证服务 G-29 安装流）→ 裁定 🔗 有据分歧，非缺口。
3. **三方法互证**：双侧 spec diff（141+1）/ 静态挂载提取（142 挂载、0 残、0 额外）/ 运行时探针（142/142 PASS）——三条独立路径同一结论。
4. **范围声明**：本 census 覆盖 opencode server API 面（packages/server + protocol HttpApi）。enterprise 代理（`routes/api/[...path].ts`）、console/desktop 均为独立部署物，属登记册既有 bounded 范围外项，不在本通道 census 内。
5. **方法论结论（最小验证通过）**：HTTP 通道在"双侧权威自报 + 生产挂载提取 + 运行时探针"三层下覆盖率 100%（142/142 逐项归属），缺口候选 0、有据分歧 1。每次扫描的边际成本 ≈ 1 条命令（脚本幂等），上游 bump 时轴 1 自动给出新增/删除端点清单。
