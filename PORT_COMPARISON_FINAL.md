# OpenCode TS → Rust 移植全面对比审查（最终汇总）

审查方法：10 条并行通道逐模块对读两侧源码（ref: /opt/opencode-ref，rust: /opt/opencode-rust），不采信仓库内任何已有审查文档；rust 代码内嵌的 ref `file:line` 溯源注释经抽验均真实。超大文件（>5k 行）采用全量符号/字面量差分 + 关键路径逐行核验。

## 一、总评

移植完成度与忠实度**异常高**：数据库/迁移、事件总线、配置六层合并、session runner 状态机、inbox 投影、压缩提示词与收缩阶梯、全部工具的模型可见字符串（逐字）、HTTP API 路由表 141/141、错误分类正则、50+ provider 装配、SSE/WS 帧协议、鉴权算法（连 JS f64/UTF-16 语义都复刻）均对齐，并配有 golden/parity 测试体系（openapi sha256 钉住、fuzzy corpus、composer/footer/screen 五套 golden）。

主要缺口集中在：**Bedrock 两条路由缺陷（高危）、MCP stdio 执行面（高危）、IDE/编辑器实时集成（高危）、插件生态兼容性断层（高危）、npm 插件安装链与 function 包（未移植）**。

## 二、分域对比表

### 1. core：database / config / bus / state / snapshot
| 功能块 | ref 实现 | rust 实现 | 一致性 |
|---|---|---|---|
| DDL 18 表 16 索引 / 48 条迁移 | drizzle-kit 快照 (schema.gen.ts) | bootstrap.rs 逐字转写，sqlite_master 文本一致 | ✅ |
| 迁移分类/legacy journal/特殊迁移 | migration.ts | migrations.rs + seed_legacy_journal；跨进程双检增强 | ✅ |
| SQLite PRAGMA/0600/事务/savepoint | drizzle+effect 桥 | rusqlite 手写，oc_core_n savepoint 对应 effect_sql | ✅ |
| v1 内容迁移 | v1-migration.bun.ts (1135行) | v1_migration.rs (1959行) + battery 测试 | ✅ |
| durable 事件提交/replay/publishAll/路由 | bus.ts (effect) | bus.rs + routing.rs（DashMap 聚合锁） | ✅ |
| 事件清单 52 durable + 40+ ephemeral | schema 分散 | registry.rs 集中注册，逐项核对 | ✅ |
| State 通用运行时 | state.ts 懒读/revision/invalidate/batch/reconcile | state_transform.rs 仅 agent 域有界子集 | ⚠️ 中 |
| kv scan/prefixEnd | kv.ts | schema.rs | ✅ |
| 快照 capture/diff/restore | git CLI 封装 | git2 (libgit2) + 手写 SHA-1 | ⚠️ 架构差（见风险#5） |
| 配置六层发现/{env}{file}/jsonc 隔离解析 | config.ts + discovery + variable | config.rs + discovery.rs + variable.rs，测试逐条覆盖 | ✅ |
| normalize 全规则（v1 迁移/provider 改名/permissions） | normalize.ts 57-485 | normalize.rs 225-1253 逐段对应 | ✅ |
| jsonc 写回保注释 | jsonc-parser modify | jsonc_edit.rs 自研 1278 行 | ✅ |
| wellknown/credential 联动 + 10min 刷新 | config.ts 145-183 | NoWellknowon 占位 trait | ❌ 缺（中） |
| 插件 storage KV | host.ts 602-629 | trait surface TODO | ❌ 缺（中） |

### 2. core：session / tool（runner）
| 功能块 | ref | rust | 一致性 |
|---|---|---|---|
| drain/advanceToStep/runStep 结算 1-13 步 | runner/llm.ts, step.ts | drain.rs, step.rs（单写者 mpsc 替代 fiber） | ✅ |
| retry 2s→10s×10 + ±20% jitter / retry-after | retry.ts | retry.rs 常量公式逐项一致 | ✅ |
| LLM 事件发布器 100ms 批量 delta | publish-llm-event.ts | publisher.rs（deadline 轮询） | ✅ |
| 压缩：触发/摘要模板/SHRINK 阶梯/原生压缩/逐次计费 | compaction.ts | compaction.rs 提示词逐字一致 | ✅ |
| inbox 45 事件级投影/promote 变换 | inbox.ts | inbox.rs | ✅ |
| run-coordinator/execution write-ahead claim | execution 系 | execution.rs（+UNWIND_TIMEOUT 60s 增强） | ✅ |
| 工具注册/管道(before→decode→leaf→after)/权限 | tool.ts+runtime.ts | registry.rs | ✅ |
| 工具输入校验 | effect/zod 全量 schema | JSON-Schema 子集校验器（oneOf/anyOf/pattern 不校验） | ⚠️ 中低 |
| edit/read/write/patch/glob/grep/shell/subagent/question/webfetch/skill 等全部内置工具 | plugin/*.ts | tool/*.rs 模型可见字符串逐字一致 | ✅ |
| 工具输出预算 UTF-16 记账/spill/7天清扫 | tool-output.ts | tool_output.rs（连 surrogate 边界都复刻） | ✅ |
| `session.generate` hook | model-request.ts 407 | dispatch_session_generate 定义但零调用 | ❌ 中（插件回归） |
| ProviderContext/路由 hook 不兼容防护 | model-request.ts 304-318 | 仅压缩路径检查，主路径缺失 | ❌ 中 |

### 3. core：其余模块
| 功能块 | ref | rust | 一致性 |
|---|---|---|---|
| shell 服务/便携扫描器(bash/zsh/posix/pwsh 2409行)/ARITY 160 项 | shell/* | shell/scan.rs 5355 行全量手写移植 | ✅ |
| shell 输出截断上限 | 读 config tool_output | 硬编码常量 | ❌ 中 |
| pty 服务/WS 协议/票据 | pty/* + @lydell/node-pty | pty.rs + portable-pty + oc-server | ✅（win ConPTY Ctrl+C 修复未见） |
| persistent-pty 13 op 状态机/daemon v7/快照 CRC32 | index.ts + daemon.ts | persistent_pty/* + oc-pty crate（本地编译，免下载） | ✅ |
| MCP 注册表五状态/OAuth/elicitation/指令聚合 | mcp/* + @mcp/client 2.0 | mcp/* + rmcp 3.7（6 项文档化 deviation） | ✅ |
| **MCP stdio 执行面** | 经 Environment spawn（沙箱/workspace，env 不跨界） | 固定本机 spawn + host env 全量合并 | ❌ 高 |
| vcs 服务 + 插件 VcsDefinition | vcs.ts | oc-server handlers（仅内置 git/hg，无插件提供方） | ⚠️ 中 |
| file-access/mutation/retention/watcher/protected | file-*.ts, @parcel/watcher | file_*.rs + notify 6 | ✅ |
| 文件模糊搜索 | fuzzysort + fff + 10s 索引 | 每请求全量 walk + 自研打分，无索引 | ⚠️ 中（性能） |
| credential/oauth 集成/copilot | DB 明文 + 插件包 | integration/* + oc-ai copilot 协议 | ✅ |
| environment 驱动/exec-defaults(exit 44/45/46) | environment/* | environment/* | ✅ |
| image Lanczos3/PNG→JPEG | photon WASM | image 0.25 crate，错误文案字节级 | ✅ |
| formatter prettier/biome 探测 | formatter.ts | formatter.rs（命令执行 seam 注入，接线待验证） | ⚠️ 中 |
| skills/instructions/reference/ripgrep(15.1.0 自管下载) | 各 ts | engine/*, tool/ripgrep_runner.rs | ✅ |

### 4. ai（packages/ai ↔ oc-ai）
| 功能块 | ref | rust | 一致性 |
|---|---|---|---|
| 全部协议：anthropic-messages/openai-chat/openai-responses/open-responses(3365行)/gemini/interactions/vertex×4/mistral/cohere/venice/meta×2/xai/alibaba×3/zai×2/minimax/moonshot×3 | 自研 effect 实现 | 自研 reqwest + Transport trait，SSE 事件名集合差分为空 | ✅ |
| bedrock SigV4/eventstream/凭证 | @aws-sdk 系 | bedrock/{sigv4,event_stream,credentials}.rs 全手写 | ✅ |
| **bedrock-converse thinking block_binding**（Claude 5.1+ adaptive+drop_block+anthropic_beta） | bedrock-converse.ts 483-492 | converse.rs 无任何痕迹 | ❌ 高 |
| **bedrock-mantle-messages route** | providers/amazon-bedrock-mantle.ts（4 组 fixture） | 缺（cache_policy 白名单留名无实现） | ❌ 高 |
| 缓存策略 4 断点/15 协议白名单/网关别名 | cache-policy.ts | cache_policy.rs + customization.rs | ✅ |
| 错误分类 38 正则/isRetryable/retry-after | provider-error.ts | provider_error.rs 正则逐条搬运（含中文） | ✅ |
| WS 通道 55min 轮换/continuation 状态机 | transport/websocket.ts | websocket.rs 1628 行 | ✅ |
| media: image 9/video 4/speech 5/transcription 5 provider | protocols/* | media/* 全覆盖 | ✅ |
| providers 目录 50+ | 全部存在，rust 额外 copilot/perplexity | ✅（缺 typesafe-ai） |
| experimental 评测(evaluation/system-one) | experimental/* | 无 | ❌ 中 |

### 5. server / client / sdk
| 功能块 | ref | rust | 一致性 |
|---|---|---|---|
| 路由表 141 endpoint | effect httpapi + protocol/groups | routes.gen.rs（openapi sha256 生成），**1:1 无缺无多** | ✅ |
| 鉴权 HMAC/cookie/Origin/Basic | auth.ts | auth.rs 逐字节兼容（复刻 JS Number.isSafeInteger） | ✅ |
| SSE 事件流 4096 队列/15s 心跳 | event-feed.ts | sse.rs | ✅ |
| pty WS 帧/replay 64KiB/close 码 | pty.ts | pty.rs | ✅ |
| pty connect-token Origin 校验 | URL.host 精确（含端口） | 只比 hostname | ⚠️ 低-中 |
| CORS allowlist/压缩/错误信封/503 readiness | process.ts | middleware.rs/app.rs | ✅ |
| client SDK | sdk 包 | oc-client 183 方法 | ✅ |
| parity harness | — | tools/parity（ref/rust 双 server golden 对比） | rust 额外 |

### 6. TUI
| 功能块 | ref | rust | 一致性 |
|---|---|---|---|
| 渲染栈 | @opentui/core + solid-js | ratatui + taffy + 自研 reactive.rs；opentui-solid 仅 tab strip（OC_TABS_OPENTUI，默认关） | ⚠️ 架构性偏离（双渲染栈并存） |
| keymap 引擎/默认键位 183 条 | @opentui/keymap | oc-tui-core keymap（254 条含迁移） | ✅ |
| routes：home/session 全命令面/滚动/分组/anchors | routes/* | store.rs/command_palette.rs 逐项命中 | ✅ |
| **IDE WebSocket 集成**（~/.claude/ide/*.lock、SSE 端口、selection 推送） | context/editor.ts 408 行 | 仅 Zed sqlite 轮询 | ❌ 高 |
| **terminal.suspend (ctrl+z)** | app.tsx 1120 | 键位表有键、无处理器（死键） | ❌ 高 |
| **tui.command.execute / tui.session.select 事件** | app.tsx 1253 | 未处理 | ❌ 中-高 |
| session.environment 上报 / session.moved | app.tsx 489/1276 | 未上报 / moved 未处理 | ❌ 中 / 低 |
| 剪贴板写 | host+terminal 双写 | 仅 OSC52（host 臂 Unsupported） | ⚠️ 中 |
| new-session-location launch/inherit | config | 未读取，恒回默认 | ⚠️ 中 |
| 组件：dialog 24 种/composer 2011 行/autocomplete/marquee/tab 栏/sidebar | component/* ui/* | 对应 rs + 五套 golden | ✅ |
| **composer 运行态 knight-rider 渐变 spinner** | ui/spinner.ts 368 行 | 仅 `esc interrupt` 文本 | ❌ 中（可见差异） |
| fuzzy 匹配 | fuzzysort 3.1.0 | 逐运算移植，分值位级一致 + corpus golden | ✅ |
| markdown/marked/diff(jsdiff) | opentui 内建 | opentui-core (marked@17/jsdiff 忠实移植) + mermaid/latex/sixel（超出 ref） | ✅ |
| 语法高亮 | web-tree-sitter 运行时下载 37 语言 | oc-syntax 编译期内嵌 20 默认 + 17 feature 门控；markdown 无 injection | ⚠️ 中 |
| theme v1 33 资产 + v2 | @opencode/theme | oc-theme 同名资产编译期内嵌，解析语义对齐 | ✅ |
| attention/audio/zed/logo/attention 音效 | 各 ts | attention.rs/sound.rs/editor_zed.rs | ✅ |
| **插件管理页 (system/plugins.tsx)** | 358 行 UI | plugin_page.rs 仅 15 行残根 | ❌ 高 |
| **桌面/权限通知 (notifications.ts)** | 77 行 | 仅 update 横幅 | ❌ 高 |
| storybook（插件开发环境 ~1800 行） | feature-plugins/storybook | 无 | ❌ 低 |
| mini 模式核心/footer 状态机/表单/权限面板 | mini/* | 全覆盖且多处增强（golden pin） | ✅ |
| demo 剧本 | 1073 行 | ~1/3 覆盖 | ⚠️ 低 |

### 7. CLI / schema
| 功能块 | ref | rust | 一致性 |
|---|---|---|---|
| 命令树（upgrade/uninstall/api/acp/auth/mcp/plugin/models/stats/mini/run/session/service/...） | effect/unstable/cli | clap 4，命令树 100% 对齐，退出码 1/130 一致 | ✅ |
| 交互提示 | @clack/prompts | dialoguer + 手写 clack 符号 | ✅ |
| rust 额外：completion/--version-rust/--force/upgrade local-file|github | — | 均已 inline 注明 | ➕ |
| schema v2 事件 107 个/part 枚举/config 20 文件 | effect Schema（encoded/decoded 双形态） | serde + schemars（wire 形态单层） | ✅ |
| **v1/legacy schema（legacy-event、v1 session 事件、models-dev、rpc、durable-event-manifest）** | schema/src/v1 等 | oc-schema lib.rs:80 明确 TODO | ❌ 中 |
| pair --url 校验时机 | parse 期拒绝 | 推迟 handler | ⚠️ 低 |
| ACP SDK | @agentclientprotocol/sdk 1.6 | vendored agent-client-protocol 3.3 | ⚠️ 版本代差（中） |

### 8. 插件系统
| 能力 | ref | rust | 一致性 |
|---|---|---|---|
| 运行时 | 进程内 bun ESM import + TSX 实时 transform（Bun plugin API） | **bun 子进程 + NDJSON JSON-RPC**（supervisor.rs 2108 行：退避重启/strike/罢免/死锁四规则/分级超时） | ⚠️ 架构差（健壮性优于 ref） |
| ctx 表面 | JSX 组件渲染进 OpenTUI 树、renderer 直通、markdown code-block 渲染器注册、slot 任意组件 | 声明式 PluginSlotSpec/DialogSpec 物化，无 JSX | ❌ 兼容性断层（高） |
| integration OAuth 方法注册/websearch/worktree/vcs transform 域/跨位置代理 | host.ts | 未对应 | ❌ 中 |
| 发现/watch 热重载/安装 pinned | plugin/* | resolve.rs/install.rs(491行) | ✅（增强） |
| **npm registry 插件安装链（arborist/pacote）** | @npmcli 系 | 缺失（`plugin add` npm 源能力受限） | ❌ 中-高 |

## 三、外部依赖映射（摘要）

| ref 依赖 | rust 对应 | 状态 |
|---|---|---|
| effect 全家桶（runtime/Schema/httpapi/cli） | tokio + 手写状态机 + serde/schemars + axum + clap | 自研替换 |
| drizzle-orm + better/bun:sqlite | rusqlite 0.32 bundled + 手写迁移 | ✅ |
| @modelcontextprotocol/client 2.0 | rmcp 3.7（拉入 reqwest 0.13，与主仓 0.12 双版本共存） | ✅ 有注释说明 |
| @opentui/{core,keymap,solid} 0.5.14 + solid-js | opentui-rust git 依赖（单人 fork rev）+ **ratatui 0.29 主渲染** | ⚠️ 双渲染栈 |
| @agentclientprotocol/sdk 1.6 | vendored =3.3.0（去 preserve_order patch） | ⚠️ 需随上游手工同步 |
| web-tree-sitter + 运行时 wasm 下载 | tree-sitter 0.25 原生 crates（20 默认+17 feature；toml/vim/kotlin/vue vendored） | ⚠️ 取舍 |
| @lydell/node-pty / bun-pty / @opencode-ai/pty | portable-pty 0.8 + 自研 oc-pty | ✅ |
| @parcel/watcher | notify 6 | ✅ |
| fuzzysort | 逐运算移植 + golden corpus | ✅ 自研 |
| diff / jsdiff / marked | opentui-core 忠实移植 / 手写 patch 算法 | 自研 |
| photon-node (WASM resize) | image 0.25 (Lanczos3) | ✅ |
| @aws-sdk 系（SigV4/eventstream） | bedrock/*.rs 全手写 | ✅（维护点） |
| google-auth-library / jose / @octokit | ring/jwt 手写；**octokit(GitHub App) 未移植** | ⚠️/❌ |
| gitlab-ai-provider | 未发现对应 | ❌ |
| @clack/prompts / uqr / picocolors | dialoguer / oc-uqr(自研) / oc-theme 自带色板 | ✅ |
| htmlparser2 / gray-matter / jsonc-parser / ignore / mime-types | 自研 HTML 解析器 / serde_yaml / json5+jsonc_edit.rs / git2+自实现 / 自研 mime 表 | ✅ |
| hono（function 包） | 未移植 | ❌ 整包 |
| rust 独有 | tokio、ratatui、crossterm、taffy、git2、vt100、jiff、dashmap、parking_lot、clap_complete、brotli-decompressor、OTel（默认关） | 均真实使用 |

## 四、风险汇总（按等级）

**🔴 高**
1. bedrock-converse thinking block_binding 默认缺失 → Bedrock Claude 5.1+ 会话中 system 变更必失败（converse.rs）
2. bedrock-mantle-messages route 整条缺失 → Mantle + Claude 不可用
3. MCP stdio 固定本机 spawn + host env 全量继承 → 沙箱/workspace MCP 语义与安全边界退化（client.rs:1049）
4. IDE WebSocket 集成整体缺失（context/editor.ts 408 行）+ terminal.suspend 死键 + session.environment 不上报（编辑器集成一组）
5. TSX 插件兼容性断层：插件不能渲染组件、无 markdown 渲染器注册 → 现有 TS 插件生态不兼容
6. TUI 插件管理页（358 行 UI 仅剩残根）与桌面/权限通知缺失
7. npm registry 插件安装链（arborist/pacote）缺失

**🟡 中**
session.generate hook 零调用；ProviderContext/路由 hook 防护缺失（主路径）；State 通用机制有界化；插件 storage KV TODO；wellknown/credential 联动占位；v1/legacy schema TODO；ACP 1.6→3.3 代差；shell 截断不读 config；文件搜索无索引；formatter 命令执行接线待验证；git2 替代 git CLI（quotepath/autocrlf/gitconfig 差异）；Vcs 插件提供方缺失；剪贴板仅 OSC52 写；new-session-location 无效；composer 渐变 spinner 缺失；markdown 无语法 injection；17 语言 feature 门控；experimental 评测缺失；generateObject 校验子集；reqwest 双版本；opentui git 依赖无 semver 保护。

**🟢 低**（略）：x-opencode-build 头、Origin 端口比较、空页 cursor null、message.get O(n)、pair URL 校验时机、bus replay 键序敏感、手写 SHA-1 冗余、workerd 分支裁剪（合理）、demo 剧本 1/3、storybook 缺失。

**rust 侧合理增强**：跨进程 bootstrap 双检、TOOL_JOIN_TIMEOUT/UNWIND_TIMEOUT、失败回合 stderr、插件监督死锁协议、websearch provider 框架、sixel/mermaid/latex 渲染、tools/parity 双实现 golden 对比、completion 命令。
