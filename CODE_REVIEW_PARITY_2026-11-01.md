> **2026-11-01 追加核验**：第三节 P0-P2 中的 5 项指控经源码复核被驳回（迁移 id 实为一致；models-dev 生产默认刷新；event-logger/account 表/BUN_BE_BUN 均已实现），1 项已由既有 G-8/G-13 追踪；确认项以 G-79..G-92 收录于 `docs/GAPS-AND-PITFALLS.md` 第八轮，详见该文档勘误节。

# OpenCode Rust 移植独立代码审查（2026-11-01）

方法：以 `/opt/opencode-ref` 为锚，按 9 个功能域并行逐行比对源码（未采信仓库内任何既有文档：PORT_*.md / FINDINGS.md / MIGRATION.md 等仅作路径线索，不作结论来源）。分域明细报告见 `/tmp/opencode/review/L1..L9-*.md`（含逐条 文件:行 证据）。

图例：✅ 一致 / ⚠️ 部分或行为差异 / ❌ 缺失 / 🔀 有意分叉或超集

## 一、总体结论

移植整体保真度**非常高**：141 个 HTTP 端点全对齐、231 条 TUI 键位 0 缺失、48 个 DB 迁移 46 个 id 完全一致、15 个工具与全部主协议（Anthropic/OpenAI/Gemini/Bedrock/Mistral/Cohere + 9 媒体协议）逐行对应，且大量以“行号注释 + golden/digest 测试”锁定契约。多处偏差是文档化的正确性修复（retry 作用域、中断清算、boot 清扫守卫、usage 保序）。真正需要处理的缺口集中在少数功能级缺失与默认值/文案差异。

## 二、分域对比总表

| 域 | 覆盖面 | 状态概览 | 代表性差异 |
|---|---|---|---|
| L1 Session/消息管线 | session 49 文件 + runner/engine | ✅ 极忠实（drain/step/publisher/compaction 近逐行） | ❌ Session.remove 不完整（孤儿子会话）；⚠️ tool.progress 不回写 publisher（live tool.failed 缺 subagent sessionID 后缀）；⚠️ http.response hook 回传 request 为空 |
| L2 Agent/Config/Skill/Model | ~50 项功能 | ✅ 高保真（加载优先级/变体协议表/技能拉取安全逐行） | ❌ `<available_skills>` 缺 permission-deny 过滤（指令面泄漏）；⚠️ skill changed 恒渲染全量；⚠️ models-dev 默认不刷新（TS fetch=true）；⚠️ options.npm provider 静默降级为 openai-compatible |
| L3 Tools | 15 工具 + 注册/校验管道 | ✅ 大多逐行对齐 | ❌ browser 插件工具整块缺失；⚠️ webfetch 超时文案/覆盖不同；⚠️ read 缺 NFC 文件名归一；⚠️ opencode.models own_provider 未接线；⚠️ patch formatter 静默容错；🔀 自研 schema 校验子集（MCP default 注入/联合类型宽松） |
| L4 MCP/Plugin/权限/凭证 | MCP×OAuth×Permission×Credential×Integration | ✅ 近乎逐行等价，测试密集 | ⚠️ MCP OAuth RFC8707 resource 拒绝分支弱化；⚠️ stdio BUN_BE_BUN 注入仅测试断言；⚠️ account 表持久化无端口；🔀 插件宿主改为 Bun 子进程 NDJSON RPC（进程内 Effect context 行为不可复刻，已知取舍） |
| L5 AI/协议 | 横切层 + Anthropic/OpenAI×2/Gemini×3/Bedrock/Mistral/Cohere/媒体 | ✅ 逐行等价 | ❌ Vercel AI Gateway gatewayProtocol（网关选项注入 + usage/cost 归账）缺失；❌ alibaba-responses 托管工具适配器缺失；⚠️ meta-responses/mantle-responses 归并到通用路径未证实 |
| L6 Server/API/Client/Schema | 141 端点 + SSE/WS + 鉴权 + 143 client 方法 | ✅ 全对齐 | ❌ oc-schema lib.rs:80 TODO（durable-event-manifest/snapshot/pty-ticket/v1/legacy/Worktree.adopt）；⚠️ 错误响应体 type 字面量未逐端点验证；🔀 Rust-only：POST /api/plugin/install、生产路由 /api/show、/api/tags |
| L7 CLI/util | 20+ 命令、全部 flag | ✅ 精确还原 | ❌ Glob.scan 无统一 API 且 match 不支持 brace/extglob；⚠️ 可执行名 opencode-rust；⚠️ fs-util/MIME 分裂、cross-spawn/npm Windows 规则未整体移植；🔀 rust-only：completion/--version-rust/upgrade local-file·github/--force |
| L8 TUI/Theme/Syntax | 键位/对话框/composer/渲染/PTY/主题 | ✅ 键位 231 条 0 缺失，主题 digest 锁定 | ⚠️ which-key 面板为“死表”（11 绑定无 UI）；⚠️ 语法高亮默认仅 14 语言、markdown 内嵌降级；🔀 7 个 rust-only 键位；🔀 Mermaid/LaTeX（>11k 行 rust-only） |
| L9 Core 基础设施 | bus/DB/KV/git/snapshot/shell/PTY/formatter 等 | ✅ 等价性高 | ⚠️ 2 个迁移 journal id 后缀不同（workspace/session，跨运行时共用 DB 会重复执行）；🔀 git 走 libgit2+CLI 双路径；⚠️ worktree 策略层不完整；❌ event-logger 缺失 |

## 三、按优先级的缺口清单

### P0（建议立即修复，正确性/数据风险）
1. **Session.remove 不完整**（L1）：缺 interrupt/awaitIdle/transport 关闭/子会话递归删除/聚合清理 → 孤儿子会话累积。`oc-core/src/session/store.rs:557`
2. **迁移 journal id 不一致**（L9）：`20260410174513_workspace`↔`..._workspace-name`、`20260511173437_session`↔`..._session-metadata`；TS/Rust 共用 DB 时重复执行。`database/migrations.rs:113+`
3. **`<available_skills>` 缺权限过滤**（L2）：被 deny 的技能仍进系统提示。`context.rs:678` vs `skill.ts:26`
4. **Vercel AI Gateway gatewayProtocol 缺失**（L5）：gateway 选项注入 + 网关 usage/cost 归账双双丢失。

### P1（用户可见行为差异）
5. tool.progress 不回写 publisher → live `session.tool.failed` 缺 `(sessionID: …)` 后缀与进度元数据（L1）。
6. which-key 面板无 UI 消费者（11 条死绑定）（L8）。
7. browser 插件工具整块缺失（若产品口径含浏览器自动化）（L3）。
8. alibaba-responses 托管工具（web_search/web_extractor/code_interpreter）缺失（L5）。
9. read 工具 alternatePath 缺 NFC 归一（Unicode 分解文件名 macOS 退化为 not found）（L3）。
10. opencode.models 的 own_provider 未回填（“Your own provider comes first” 失效）（L3）。
11. oc-schema TODO 块：durable-event-manifest / snapshot store / pty-ticket / Worktree.adopt（L6）。
12. MCP OAuth RFC8707 resource 拒绝分支弱化（L4）。
13. webfetch 超时错误文案与 ref 不同（模型可见反馈不一致）（L3）。

### P2（默认值/策略/运维面）
14. models-dev catalog 默认不刷新（TS 默认 fetch=true）（L2）。
15. patch formatter 阶段静默 continue（ref 报错）（L3）。
16. http.response hook 的 request 回传为空（L1）。
17. event-logger 缺失（仅日志行为）（L9）；worktree 策略层覆盖不完整（L9）；account 表持久化无独立端口（L4）。
18. MCP stdio `BUN_BE_BUN=1` 生产路径未显式注入（L4）。
19. Glob 语义子集（无 brace/extglob、无统一 scan API）（L7）；MIME/fs-util 分裂（L7）；Windows spawn/npm 规则缺失（L7）。
20. 语法高亮默认 14 语言、markdown 内嵌降级（L8）。

### 🔀 已知有意分叉（需验收口径确认，非缺陷）
- 插件宿主：进程内 import → Bun 子进程 NDJSON RPC（Bun 缺失时整体禁用）。
- git：libgit2（snapshot）+ CLI（project/vcs）双路径。
- 动态 npm provider 加载 → 静态协议集 + typed `UnsupportedProtocol`。
- Rust-only 超集：CLI completion/--version-rust/--force/upgrade local-file·github、/api/plugin/install、7 条 TUI 键位、Mermaid/LaTeX 渲染、>20 格式化器扩展、输入修复模块 tool_input_repair.rs。
- 多处为修复 ref 缺陷的增强：edit pass2 偏移映射、retry 作用域、boot 清扫守卫、title 乐观并发重试、TOOL_JOIN_TIMEOUT 兜底。

## 四、残余核对盲区（本轮未逐行，建议补查）
- codemode.rs（2300 行）、html_markdown.rs（2023 行）、tool_output.rs、mcp/registry.rs（2000 行）为抽样一致（L3）。
- oc-client 184 fn 的逐字段签名（L6）；错误响应体 type 字面量逐端点（L6）。
- mini 子系统 footer 各变体语义（44→16 文件合并）（L8）。
- meta-responses / bedrock-mantle responses 适配差异（L5）。
- session environment GET 路由、backgroundAll 文案、transfer sanitize 逐字段（L1）。
