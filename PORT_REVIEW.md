# opencode TS → Rust 移植全量对比审查（2026-10-27）

> **状态（2026-10-28）**：本文件 §3 优先修复清单 11 项及 RV 系列全部缺口已经 rv-burndown 收口——B1..B9/G1..G4/P1/P4..P9 实现闭环，G5/G6/P10..P14 具名豁免（双侧证据裁定，.slim/deepwork/rv-burndown/rulings-4.md）。逐项处置与验证凭据见 docs/GAPS-AND-PITFALLS.md §4 头注。下文为审查时点快照。
方法：7 个并行通道逐域对比 /opt/opencode-ref 与 /opt/opencode-rust，逐文件清点 + 关键路径行级核对，不采信既有迁移文档。Rust 侧大量文件带 REF 行号锚点注释，抽验属实。

## 1. 总体结论

移植完成度很高。141 个 HTTP 端点零缺失零多余、CLI 18 个子命令全量对等、15 个内置工具基本 byte-exact、AI 协议（含 23 个媒体协议）全覆盖且零 AI SDK 依赖、核心事件引擎（bus/projector/inbox/drain/compaction）高保真。

## 2. 功能域对比总表

| 功能域 | REF | Rust | 一致性 | 关键差异 |
|---|---|---|---|---|
| Session 引擎（projector/inbox/执行协调/drain/step/retry/publisher/compaction/title/context） | packages/core/src/session | oc-core/session + engine | 🟢 高保真 | 见 §3 缺陷 1-6 |
| 消息/会话持久化、fork/revert/diff/move | session/*.ts | store/fork/revert/diff | 🟢 | pathdiff 同路径 `"."` vs `""`；compaction spent 清空范围过大 |
| AI Provider/协议 | packages/ai | oc-ai | 🟢 | experimental/evaluation 543 行未移植；未移植协议走类型化 UnsupportedProtocol（REF 为 aisdk: 动态兜底）；sanitize 为文档化 no-op |
| models.dev / catalog / resolver / copilot | core 各文件 | provider_catalog | 🟢 | 快照内嵌 + KV 刷新 + bus 事件对齐 |
| 内置工具 15 个 | core/src/tool | oc-core/src/tool | 🟢 | question multiple:true 未映射 multiselect（退化）；grep/glob/read 结构化 Output 未导出；shell 描述不随 hook 刷新 |
| 工具注册/管线/输出预算 | tool.ts/tool-output.ts | registry.rs/tool_output.rs | 🟢 | JSON-Schema 校验器为子集（无 pattern/additionalProperties） |
| HTTP Server | packages/server | oc-server | 🟢 | /api/info wildcard 地址不展开；事件公开性为手工 registry（漂移风险） |
| SDK/Client | packages/sdk+client | oc-client | 🟡 | 141 方法+SSE 解码全对齐；**无重连/共享订阅/响应式层** |
| Schema | packages/schema+protocol | oc-schema | 🟡 | 长尾缺：snapshot/llm/money/token-usage/durable-event 系列 |
| CLI | packages/cli | oc-cli | 🟢 | 3 处 rust-extra（completion、upgrade method、--force）；Effect CLI→clap（校验前移） |
| TUI | packages/tui | oc-tui(+core) | 🟢 | OpenTUI/Solid → opentui-rust+ratatui+taffy 混合；50+ 组件折叠为通用框架，靠 golden/PTY 测试对齐 |
| ACP | cli/src/acp | oc-cli/src/acp | 🟢 | 文件 1:1（自研 JSON-RPC 替代 @agentclientprotocol/sdk） |
| 配置（发现/合并/JSONC/热重载/variable） | core/config | oc-core/config | 🟢 | jsonc 保格式编辑为自研重写 |
| 指令/技能/Agent | instructions/skill/agent | engine/instructions、skills | 🟢 | skill 端点只准入占位、正文未进上下文（缺陷 3） |
| MCP（stdio+StreamableHTTP/OAuth/elicitation） | core/mcp | oc-core/mcp（rmcp） | 🟢 | — |
| 插件系统 | plugin/host（in-process Bun） | oc-plugin-host（Bun 子进程 NDJSON RPC） | 🟡 架构差异 | 行为对齐（崩溃隔离增强）；effect 轨插件 SDK 无等价物 |
| 主题/语法高亮 | theme + WASM tree-sitter | oc-theme + oc-syntax（原生 tree-sitter，~30 语言） | 🟢 | 33 内置主题内嵌；scope 规则对齐 |
| Formatter | formatter/builtins.ts（26 工具链） | NoopFormatter stub | 🔴 **缺失** | 写后自动格式化完全不生效 |
| 文件监听 | filesystem/watcher（@parcel/watcher） | 仅 config 热重载（notify） | 🔴 **缺失** | FileSystem.Event.Changed 无发布者，location 活动检测缺失 |
| 图片处理 | image.ts（photon WASM 缩放） | 无 | 🔴 **缺失** | — |
| 事件总线 Bus | bus.ts（Effect+drizzle） | bus.rs（tokio+rusqlite） | 🟢 | 事件溯源不变量逐条落地 |
| 数据库/KV | drizzle + 三 SQLite 后端 | rusqlite bundled 手写 SQL | 🟢 | v1 迁移电池测试齐全 |
| 权限系统 | permission.ts | permission.rs | 🟢 | — |
| PTY / 持久 PTY | node-pty / daemon | portable-pty / 进程内状态机+oc-pty daemon | 🟡 | daemon 接线（Phase 9）未完成，跨重启存活体验有差距 |
| Git/VCS/worktree | git.ts/vcs.ts/simple-git | git2 + git CLI + project.rs | 🟡 | VCS 抽象层、gitconfig include、worktree 策略层未移植（功能大半被吸收） |
| Websearch | websearch.ts | tool/websearch*（5 provider） | 🟢 | MCP 托管 provider TODO |
| Identity/OAuth | oauth/（授权码+PKCE+设备流） | mcp/oauth.rs（授权码+PKCE） | 🟡 | 设备流未见实现 |
| CodeMode | 进程内受限 JS 解释器 | oc-plugin-host 沙箱 | 🟡 架构差异 | 限额在桥上对齐（已声明 wave-4 分歧） |
| 云函数验证 | packages/function | 无 | 🔴 缺失 | 低影响 |
| 长尾 provider 适配器（poe/snowflake/digitalocean…） | plugin/provider ~30 个 | 协议族归并 | ⚠️ 未逐个确认 | 建议专项复核 |
| experimental/evaluation | packages/ai/experimental | 无 | 🔴 缺失 | 543 行 |

## 3. 优先修复清单（按严重度）

1. 🔴 Formatter 26 个工具链（写后格式化失效）
2. 🔴 通用文件监听服务（filesystem events + location 活动）
3. 🔴 runStep 步数计数：drain.rs:741 重试/Continue 也 `step+=1`，REF 重试不耗步数（疑似移植 bug）
4. 🔴 session import 为占位（导入内容全丢）
5. 🟡 prompt/compact 前未 commit staged revert
6. 🟡 skill 激活正文未注入上下文
7. 🟡 model-transport（会话 WebSocket 传输）仅 seam
8. 🟡 background 端点空 204（REF 注入合成通知）
9. 🟡 question multiple:true → multiselect
10. 🟡 image.ts photon 缩放缺失
11. 🟡 OAuth 设备流、持久 PTY daemon 接线、client 重连层、/api/info wildcard 展开

## 4. 外部依赖替代总表

| REF (TS) | Rust |
|---|---|
| Effect（运行时/DI/CLI/HttpApi/Schema） | tokio + Arc 显式服务 / clap 4 / axum+tower / serde+oc-schema |
| drizzle-orm + better-sqlite3/bun:sqlite | rusqlite（bundled）手写 SQL |
| Vercel AI SDK / @ai-sdk/* / openai | 全自研（reqwest + 手写协议状态机 + 磁带回放测试） |
| @parcel/watcher | notify 6（仅 config 域） |
| simple-git | git2 0.19 + git CLI |
| node-pty | portable-pty 0.8 |
| @modelcontextprotocol/sdk | rmcp 3.7 + reqwest transport |
| @opentui/core/solid/keymap | opentui-rust（git rev 固定）+ ratatui + taffy + crossterm |
| @agentclientprotocol/sdk | 自研 ACP JSON-RPC |
| jsonc-parser | json5 + 自研 jsonc_edit.rs |
| web-tree-sitter（WASM） | 原生 tree-sitter 0.25 crate 集 |
| npm diff / htmlparser2 / photon-wasm | 手写 LCS diff / 手写 tokenizer+MD / ❌ 无 |
| Bun 运行时（插件） | 仍用 Bun，改为托管子进程（pinned 1.3.10） |
| zod/effect Schema 校验 | serde + 手写 normalize 诊断 / JSON-Schema 子集校验器 |

## 5. 各通道详细报告索引

- Session/Engine：本文件历史（审查含 40+ projector 事件逐条对照）
- AI/Provider：~90k 行对照，媒体 23 协议 1:1
- Tools：15 工具逐个（见通道报告）
- Server/API/SDK：141 端点全量 diff + 外壳逐行
- CLI/TUI：18 子命令 + 50 组件 + ACP 1:1
- 配置/插件/MCP/主题：config 六源、MCP、theme 全管线逐行
- 基础设施：bus/kv/permission/pty/ripgrep/websearch 行级
