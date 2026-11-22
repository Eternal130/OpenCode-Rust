# 与 ref 的架构对比（opencode v2.0.24：TypeScript 参考实现 → Rust 移植）

一句话总览：ref 是 bun monorepo（`packages/*`，TS），rust 是 cargo workspace 14 crates（源码 53.9 万行，2026-11-22 实测 `crates/*/src`），逐包映射并对每个结构性分歧做文档化裁决（见 `DIVERGENCES.md`）。

## 两树拓扑

| | ref（TypeScript） | rust（移植） |
|---|---|---|
| 构建 | bun monorepo，`packages/*` | cargo workspace 14 crates（`crates/*`） |
| 规模 | TS ~24.7 万行核心链路 | 源码 539,175 行（src，不含 tests） |
| 测试 | vitest / bun-test | nextest 四层门 + 12 guards + parity 334 金帧 + realchain 四目标电池 |

## 分层映射（ref package ↔ rust crate）

| ref package | rust crate（src 行数，2026-11-22 实测） | 说明 |
|---|---|---|
| core | oc-core（184,989）+ oc-ai（79,646） | 拆分：LLM SDK（providers/protocols/stream）独立为 oc-ai |
| tui | oc-tui（211,299）+ oc-tui-core（4,778）+ oc-plugin-host（16,352） | oc-tui-core=跨 crate 契约；oc-plugin-host 含 ref supervisor 职责（bun guest 进程管理 + TSX shim 桥） |
| server | oc-server（51,985）+ oc-http（394） | oc-http 抽出 TLS/信任栈（platform-verifier + webpki 降级） |
| cli | oc-cli（31,897）+ oc-pty（4,563） | oc-pty 为独立 PTY 守护进程（见下） |
| sdk / client | oc-client（22,037） | 生成式：`tools/openapi-gen` 从 sha256 钉死的 openapi.json 生成 |
| theme | oc-theme（4,068） | |
| schema（内嵌 Effect Schema） | oc-schema（9,356） | 手写 serde + schemars，goldens 防漂（G-16） |
| util | oc-util（5,176） | |
| 语法高亮（tree-sitter 内嵌） | oc-syntax（1,880） | grammar+query vendor；高亮引擎走 opentui TreeSitterClient |
| —（ref 无对应） | vendored：ACP×2、tree-sitter grammar×4、oc-uqr（`third_party/`） | ACP 带 serde 差异 patch |

## 关键结构决策（决策 → 为什么 → 分歧指针：G 号条目见 `DIVERGENCES.md`）

1. **渲染引擎**：ref 内嵌 opentui（Solid TSX）→ rust 用跨仓 opentui-rust（git rev `0f7fb04` 钉扎，opentui-core/keymap/solid）+ oc-tui。防漂手段=双引擎金帧 oracle + 树场景 334 金帧差分（`tools/parity/`）。→ 已收口（历史缺口 G-1..G-464 全清；现状见 `DIVERGENCES.md`）
2. **插件运行时**：ref per-Location worker → rust 全进程单 bun guest（G-289）+ TSX 有界 shim：渲染树跨 IPC 拍平为 `{title,rows,clickable}` 行形状；markdown 渲染器注册链与 panel slot 已落地，声明面收窄（G-28）。
3. **HTTP 服务**：ref workerd 可嵌入（fetch.ts/workerd.ts）→ rust 原生 axum `boot_and_serve`，无 JS 运行时嵌入目标（G-163）。
4. **Schema**：ref Effect Schema → 手写 serde + schemars + golden 对拍防漂；`/openapi.json` sha256 钉死、升级走人工 baseline-bump（G-16，长期漂移维护坑）。
5. **客户端 SDK**：ref 手写 sdk → rust `tools/openapi-gen` 从钉死 openapi.json 生成 oc-client（wire 形状由源头单一事实保证）。
6. **PTY**：oc-pty 独立守护进程（Unix socket 通信；历史设计 `docs/superpowers/specs/2026-10-23-pty-daemon-design.md`，已出树 git 可溯）；override 路径属主加固、免 sha256 钉扎为文档化取舍（G-240）。
7. **vendored**：`third_party/` ACP×2（agent-client-protocol / -schema，serde 差异 patch）、tree-sitter grammar×4（kotlin/toml/vim/vue）、oc-uqr——钉死上游、patch 有据。
8. **验证体系**：ref vitest/bun-test → nextest 四层门（常设门→差分→合并门→pty 序列化）+ 12 guards（`tools/guards/run.sh`）+ parity 334 金帧 + realchain 四目标电池与控制者裁决注册表（`tools/diff/realchain/adjudications.json` + `adjudicate.py`）。详见 `AGENTS.md` 开发规范 §2.2。

## 构建与工具链

- Rust `1.99.0` 钉扎（`rust-toolchain.toml`）；仓库级 incremental=false 强制。
- `tools/dev-gate.sh` 单入口：systemd scope（CPUQuota=400% / MemoryMax=6G / TasksMax=512）+ load/磁盘闸；验证脚本（diff/parity/guards）直跑禁包门。
- CI 四流水线（`.github/workflows/`）：`ci.yml` / `runtime-gates.yml` / `release.yml` / `realchain-diff.yml`。
- 差分工具族：`tools/diff/*.py`（HTTP/SSE/JSON-SCHEMA/ACP/TUI 整屏）、`tools/parity/`（金帧）、`tools/diff/realchain/`（真链路电池+裁决）。

## 指针

- 已知分歧登记册 → `DIVERGENCES.md`；开发规范 → 本地私有册 `docs/GAPS-AND-PITFALLS.md`（git-ignored）
- agent 入口（规范权威源 `tools/lane-brief.sh`）→ `AGENTS.md`
- 历史 ADR / 审计台账 / 设计文档 → 本地仓外归档（不入库；完整判定史不随公开仓分发——2026-11-22 隐私清理已历史重写；全文副本=本地备份与仓外归档）
