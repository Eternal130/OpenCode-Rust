# opencode v2.0.24 → Rust 移植独立审查汇总（2026-10-25）

> 四条并行审查泳道（core / server / TUI / AI+插件+CLI+分发），均以 `/opt/opencode-ref`（v2.0.24 钉版）与 `/opt/opencode-rust` 实际代码独立核实，不采信 PORT_REPORT 自述。评级：一致 / 等价（实现方式不同、行为等价）/ 部分 / 缺失 / 刻意分歧（已登记）。

## 0. 总体结论

| 域 | 评级 | 一句话结论 |
|---|---|---|
| core（session/agent/工具/DB/MCP） | 等价 | 48 条迁移、16 工具、permission、compaction、revert、MCP+elicitation 一一对应；工具 schema 逐字节；无虚报 |
| server/protocol/client | 一致 | 141↔141 路由程序化对账全等；鉴权与 SSE 为最高保真区；最大缺口=仅 2/141 路由 schema 校验 |
| TUI | 等价 | 对话框族 50+ 变体、mini、acp、worktree、fuzzy 逐位；无缺失项；leader 和弦为唯一键位级分歧 |
| AI/插件/CLI/分发 | 等价 | CLI 命令面逐 flag 忠实；provider 表驱动等价；插件宿主刻意分歧换崩溃隔离 |

## 1. 未登记分歧 / 报告滞后（本次审查新发现，⚠ 建议补登记）

| # | 发现 | 域 | 性质 |
|---|---|---|---|
| N1 | 包管理器升级路径（npm/pnpm/bun/yarn/vp）降级为统一报错，ref 会真实执行 `npm install -g` 等（`oc-cli/services/updater.rs:85,201`） | CLI/分发 | 未登记行为降级 |
| N2 | oc-theme 的 discovery / system-theme（系统深浅色跟随）明确 intentional not ported（`oc-theme/src/lib.rs:14`） | theme | 未登记缺失（GUI 侧影响，TUI 影响小） |
| N3 | `event-logger.ts`、`file-retention.ts` 在 rust 全仓无独立对位模块——需确认被 bus/DB 吸收还是漏移植（后者有磁盘增长风险） | core | 待核实的疑点 |
| N4 | OS 通知通道：ref 走宿主 renderer 回调，rust 走 OSC 9/777/99；不支持该序列的终端（裸 console/部分复用器）将静默丢失通知 | TUI | 未登记协议选择分歧 |
| N5 | openai-responses WebSocket 通道**已实现**（`ws-transport` 在默认 feature），PORT_REPORT §3 仍写"延后" | AI | 报告滞后，非缺口 |
| N6 | oc-syntax 语言查询覆盖面与 ref `parsers-config.ts` 未逐项对表；缺语言退化为纯文本着色 | TUI | 中风险待补对表 |
| N7 | openapi 由 ref 运行时生成改为钉死快照伺服（含 routes/validation/types/methods 四生成物同锁）；对 ref 演进是快照滞后型风险 | server | 登记不充分 |
| N8 | 错误信封（~28 类）为手工注册表移植，无生成对账；ref 新增错误类会静默退化为 UnknownError | server | 维护性风险 |
| N9 | worktree 策略枚举（ref `worktree/strategies.ts`）被压平进 oc-server handlers，策略对应关系未逐项验证 | core | 建议补一次对表 |

## 2. 已登记且核实属实的刻意分歧（摘要）

- 插件宿主 bun 子进程 + hook copy-in/copy-out（R1–R5 永久备案）
- 独立身份：二进制/XDG 目录改名 + v2 存量单向拷贝迁移
- 更新通道：自有 GitHub feed + 强制 sha256（上游无校验，属加固）
- oc-pty 独立守护 + 跨重启 adopt（超出上游的超集）
- 键位/主题解析 throw→skip（v2 会崩，属加固）
- leader 和弦在非空草稿不触发（唯一键位级行为差异，仍开放）
- 请求 schema 校验仅 session.create/list 2 条路由（P10-N1，本域最大契约缺口）
- LSP：双侧共同缺失（ref v2.0.24 本就只有 schema 无运行时），非缺口但不得宣称可用

## 3. 分域对比表

### 3.1 core 域（packages/core ↔ oc-core/oc-util/oc-schema）

| 功能项 | ref | rust | 评级 |
|---|---|---|---|
| session 生命周期/持久化 | session/ + drizzle sqlite | engine/session 系 + 48 条迁移转译（stepwise 电池 k=1..48 全绿；真实历史库门 OC_P10 未过） | 等价 |
| agent 循环/runner | session/runner | 拆 14 文件，注释逐行溯源 | 一致 |
| 工具集（16 个） | tool/*.ts | engine/tool/，schema 与描述逐字节 | 一致 |
| permission/授权工具 | permission/ | engine/permission | 一致 |
| compaction / revert / summarize | session 内 | 对应模块 | 一致 |
| MCP（客户端+服务端、elicitation） | mcp/ | rmcp 3.7 + elicitation（R-10 已闭环） | 一致 |
| LSP | 仅 ConfigLSP schema，无运行时 | 同（双侧共同缺失） | 一致(缺失) |
| config（加载/合并/JSONC 写回） | config/ | config/ + jsonc_edit.rs（D-01 保留注释改写） | 一致 |
| auth/models registry | auth.ts / models-dev.ts | auth + provider_catalog 钉版快照 + CACHE_KEY 刷新 | 一致 |
| 事件总线/projector | bus/projector | bus + projector 重放对账（165 事件逐列相等） | 一致 |
| skills/command/instructions | skill/ + instructions/ | engine/{skills,skills_url,commands,instructions}（URL 源原子刷新） | 一致 |
| plugin API | plugin/ ~30 文件 | oc-plugin-host 跨 crate，R1–R5 刻意分歧备案 | 等价 |
| event-logger / file-retention / kv | 独立模块 | kv 在 server 侧；前两者无对位（见 N3） | 部分 |
| pty / persistent-pty | 进程内 node-pty | oc-pty 守护 + adopt + wire golden | 等价(超集) |
| vcs/git | git.ts/vcs.ts | 散入 server handlers，wire 平价测试在 | 部分等价 |
| worktree | core/strategies.ts | 压平进 server handlers（见 N9） | 部分 |

### 3.2 server/protocol/client 域

| 功能项 | ref | rust | 评级 |
|---|---|---|---|
| 路由表面 141 操作 | effect HttpApi 声明 | routes.gen 静态表，程序化对账全等 | 一致 |
| openapi 伺服 | 运行时生成 | 钉死快照 + sha256 防漂（见 N7） | 等价 |
| 错误形状 ~28 类 | TaggedError | 手工注册表（见 N8） | 一致 |
| 请求校验→400 | 全路由自动 decode | 仅 2/141 接线（P10-N1） | 部分 |
| SSE /api/event + session log | handleRaw + 心跳 | 逐项含失败模式移植 | 一致 |
| SSE id:/Last-Event-ID | 服务器不支持 | 同；客户端多发头（已登记） | 刻意分歧 |
| 鉴权（Basic/cookie/auth_token/pairing） | HMAC 双跳 + timingSafeEqual | 逐语义（含 f64/base64 边缘复刻） | 一致 |
| serve/--service/readyiness 503 | serve.ts | 三模式同路径 + code 键特例对齐 | 一致 |
| /api/info、location 三级解析 | server-info/location.ts | info.rs + middleware 同优先级 | 一致 |
| rpc.call / codemode 暴露 | rpc + codemode | routes.gen + 令牌门 | 等价 |
| SDK/client | httpapi-codegen 生成 | openapi-gen 生成，141 operationId 对齐 | 等价 |

### 3.3 TUI 域

| 功能项 | ref | rust | 评级 |
|---|---|---|---|
| 架构/渲染 | solid-js + @opentui | 自建 reactive + opentui-rust(钉版 bee57db) | 等价 |
| tabs / transcript / grouping / anchor | session-tabs 等 | components/ + golden Tier-A 逐字节 | 一致 |
| markdown / diff | plugin/markdown | markdown.rs(C1) / diff(Tier-B) | 一致(降级锚) |
| 图片/kitty/sixel | dialog-image-preview | image.rs + 协议（无 ref 侧字节锚） | 一致 |
| composer 五态 / $EDITOR / 粘贴 | prompt/ | composer/ + paste 部分（preventDefault 未移植，已登记） | 一致/部分 |
| footer/keymap/leader 和弦 | keybind.ts | keymap.rs（非空草稿和弦不触发，已登记） | 部分 |
| 对话框族（25+）/fuzzy | dialog-* | DialogKind 50+ 变体 + fuzzysort 源码级移植（f64 逐位） | 一致 |
| 主题/语法高亮 | theme/ + tree-sitter | oc-theme 63 主题逐字节 + oc-syntax（覆盖面对表待补 N6） | 一致 |
| 通知（声音/OS） | renderer 回调 | 5 音 + OSC 9/777/99（N4） | 等价 |
| mini / acp / embedded terminal / PTY | mini/ acp/ terminal-pane | 全对应 + oc-pty 超集 | 一致/超集 |
| 启动/重启恢复/安全关机/worktree UI/子代理表面/命令面板 | 各组件 | 全对应（M2 23 步实测） | 一致 |
| golden/parity 体系 | 无 | 37 场景/66 变体 + tui-dump --verify | 刻意分歧(资产) |

### 3.4 AI/插件/CLI/分发域

| 功能项 | ref | rust | 评级 |
|---|---|---|---|
| Provider 注册表（49 模块） | providers/index.ts | native.rs 表驱动 + 21 protocol（抽查映射正确） | 等价 |
| models.dev 元数据 | 运行时拉取 | 钉版快照 + 同语义刷新 | 一致 |
| anthropic/openai(chat/responses)/google/vertex/azure/bedrock/cohere/mistral/xai/copilot 等 | protocols/*.ts | protocols/*.rs + parity 测试；bedrock sigv4 自实现 | 一致 |
| openai-responses WebSocket | 生产通道 | 已实现（N5 报告滞后） | 一致 |
| 媒体四族（image/speech/video/transcription） | media 协议 | oc-ai/media/ 目录级对应 | 一致 |
| cassette 重放 | 51 测试文件 | 18 个聚合 parity + ≥274 盘语料断言 | 等价 |
| 插件宿主/V1V2//api/plugin/CodeMode | 进程内 + codemode 包 | bun 子进程 + guest 解释器（指令钉字节） | 等价(刻意分歧) |
| CLI 子命令面（20 命令树） | commands.ts | 逐命令逐 flag + 解析 parity 单测（rust-extra 已标注） | 一致 |
| 更新通道 | 多包管理器自更新、无 sha256 | 自有 feed + 强制 sha256；包管理器路径报错拒驱（N1） | 部分(刻意) |
| install.sh / bun 分发 | 官方探测 | 参数化 + 拒绝超基线探测 + 钉版 bun 托管下载 | 等价(刻意) |
| theme / function / web 等 | — | discovery 未移植（N2）；function=CF Worker 云服务、web/desktop 等确认范围外 | 范围外/部分 |

## 4. 建议行动项（按优先级）

1. 核实 N3：确认 event-logger / file-retention 是否被吸收或漏移植（磁盘增长风险）。
2. 补登记 N1/N2/N4 到 GAPS-AND-PITFALLS；修订 §3 的 WebSocket"延后"表述（N5）。
3. 补两张对表：oc-syntax 语言集 vs parsers-config（N6）；worktree 策略枚举（N9）。
4. 为图像/mermaid 增加 ref 侧真机锚（TUI 发现 #9）。
5. 错误信封与请求校验（P10-N1）考虑生成化对账，消除快照滞后与静默退化（N7/N8）。
